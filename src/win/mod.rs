//! Win32 연결부: 훅 스레드, 입력 주입, 워커, 실행 진입점.

mod clock;
mod hook;
mod input;
mod worker;

use std::ptr::null;
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::sync::mpsc;
use std::thread;
use std::time::Duration;

use glide_core::gesture::{DEFAULT_THRESHOLD, MAX_THRESHOLD, MIN_THRESHOLD};
use windows_sys::Win32::Foundation::{ERROR_ALREADY_EXISTS, GetLastError, TRUE};
use windows_sys::Win32::System::Console::{
    CTRL_CLOSE_EVENT, CTRL_LOGOFF_EVENT, CTRL_SHUTDOWN_EVENT, SetConsoleCtrlHandler,
};
use windows_sys::Win32::System::Threading::CreateMutexW;
use windows_sys::Win32::UI::HiDpi::{
    DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2, SetProcessDpiAwarenessContext,
};
use windows_sys::Win32::UI::WindowsAndMessaging::{PostThreadMessageW, WM_QUIT};
use windows_sys::core::BOOL;

/// 실행 설정(M1은 명령줄 인자로만 받는다).
#[derive(Clone, Copy, Debug)]
pub struct Config {
    /// 방향 판정 임계값(96 DPI 기준 px, 12~48)
    pub threshold: u32,
    /// 움직이지 않고 이 시간(ms) 동안 누르고 있으면 원래 우클릭으로 통과
    pub hold_ms: u32,
    /// 지연 통계 출력 주기(초). 0이면 종료할 때만 출력
    pub stats_sec: u64,
}

impl Default for Config {
    fn default() -> Self {
        // hold_ms 기본값은 M1 실측으로 정할 임시 값이다.
        Self {
            threshold: DEFAULT_THRESHOLD,
            hold_ms: 400,
            stats_sec: 30,
        }
    }
}

const USAGE: &str = "\
사용법: glide-core [옵션]

  --threshold <12~48>   방향 판정 거리(96 DPI 기준 px, 기본 22)
  --hold-ms <100~3000>  움직이지 않고 누르고 있으면 원래 우클릭으로 통과하는 시간(기본 400)
  --stats-sec <초>      훅 처리 시간 통계 출력 주기(기본 30, 0이면 종료 때만)
  -h, --help            이 도움말

M1 프로토타입: 우클릭한 채 왼쪽으로 그리면 뒤로 가기(Alt+←)를 보냅니다.
Shift·Ctrl·Alt·Win을 누른 채 우클릭하면 제스처 없이 원래 우클릭이 됩니다.
Ctrl+C로 종료합니다.";

fn parse_args(args: impl Iterator<Item = String>) -> Result<Option<Config>, String> {
    let mut config = Config::default();
    let mut args = args.peekable();
    while let Some(arg) = args.next() {
        let mut number = |name: &str, min: u64, max: u64| -> Result<u64, String> {
            let value = args.next().ok_or_else(|| format!("{name} 값이 없습니다"))?;
            let parsed: u64 = value
                .parse()
                .map_err(|_| format!("{name} 값이 숫자가 아닙니다: {value}"))?;
            if !(min..=max).contains(&parsed) {
                return Err(format!("{name} 값은 {min}~{max} 사이여야 합니다: {parsed}"));
            }
            Ok(parsed)
        };
        match arg.as_str() {
            "--threshold" => {
                config.threshold =
                    number("--threshold", MIN_THRESHOLD as u64, MAX_THRESHOLD as u64)? as u32
            }
            "--hold-ms" => config.hold_ms = number("--hold-ms", 100, 3_000)? as u32,
            "--stats-sec" => config.stats_sec = number("--stats-sec", 0, 86_400)?,
            "-h" | "--help" => return Ok(None),
            other => return Err(format!("알 수 없는 옵션입니다: {other}")),
        }
    }
    Ok(Some(config))
}

/// 종료 신호를 보낼 훅 스레드 ID
static HOOK_THREAD: AtomicU32 = AtomicU32::new(0);
static STOPPING: AtomicBool = AtomicBool::new(false);

/// 정상 종료 경로(마지막 통계 출력)까지 마쳤는지
static FINISHED: AtomicBool = AtomicBool::new(false);

/// Ctrl+C·창 닫기 처리기. 훅 스레드의 메시지 루프를 끝내 정상 종료 경로를 탄다.
unsafe extern "system" fn on_console_signal(signal: u32) -> BOOL {
    STOPPING.store(true, Ordering::SeqCst);
    let thread_id = HOOK_THREAD.load(Ordering::SeqCst);
    if thread_id != 0 {
        // SAFETY: 훅 스레드 ID에 종료 메시지를 넣는다. 스레드가 이미 끝났으면 실패만 한다.
        unsafe { PostThreadMessageW(thread_id, WM_QUIT, 0, 0) };
    }
    // 창 닫기·로그오프·종료는 이 처리기가 돌아가는 즉시 Windows가 프로세스를 끝낸다.
    // 마지막 통계가 출력되도록 정상 종료를 잠시(최대 3초) 기다린다. Windows가 주는 유예는 약 5초다.
    if matches!(
        signal,
        CTRL_CLOSE_EVENT | CTRL_LOGOFF_EVENT | CTRL_SHUTDOWN_EVENT
    ) {
        for _ in 0..60 {
            if FINISHED.load(Ordering::SeqCst) {
                break;
            }
            thread::sleep(Duration::from_millis(50));
        }
    }
    TRUE
}

fn wide(text: &str) -> Vec<u16> {
    text.encode_utf16().chain(std::iter::once(0)).collect()
}

fn print_stats(label: &str) {
    println!(
        "[{label}] {} · 재설치 {}회 · 버린 사건 {}",
        hook::HOOK_LATENCY.snapshot().summary(),
        hook::REINSTALLS.load(Ordering::Relaxed),
        hook::DROPPED_EVENTS.load(Ordering::Relaxed)
    );
}

/// 실행 진입점. 프로세스 종료 코드를 돌려준다.
pub fn run() -> i32 {
    let config = match parse_args(std::env::args().skip(1)) {
        Ok(Some(config)) => config,
        Ok(None) => {
            println!("{USAGE}");
            return 0;
        }
        Err(message) => {
            eprintln!("{message}\n\n{USAGE}");
            return 2;
        }
    };

    // 같은 사용자 세션에서 두 번 실행되면 우클릭을 이중으로 가로채므로 막는다.
    // 핸들은 프로세스가 끝날 때까지 유지해야 하므로 닫지 않는다.
    let name = wide("Local\\GlideWindowsGesture.Core");
    // SAFETY: 널 보안 속성과 널 종료 UTF-16 이름을 넘긴다.
    let mutex = unsafe { CreateMutexW(null(), 0, name.as_ptr()) };
    if mutex.is_null() || unsafe { GetLastError() } == ERROR_ALREADY_EXISTS {
        eprintln!("이미 실행 중입니다.");
        return 1;
    }

    // 훅 좌표를 물리 픽셀로 받고 모니터별 DPI로 임계값을 맞추기 위해 Per-Monitor v2로 동작한다.
    // SAFETY: 프로세스 시작 직후 한 번 호출한다. 실패해도(구버전 Windows) 계속 실행한다.
    unsafe { SetProcessDpiAwarenessContext(DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2) };

    let (event_tx, event_rx) = mpsc::sync_channel(1024);
    let (ready_tx, ready_rx) = mpsc::channel();
    let worker = thread::Builder::new()
        .name("glide-worker".into())
        .spawn(move || worker::run(event_rx));
    let hook_thread = thread::Builder::new()
        .name("glide-hook".into())
        .spawn(move || hook::run(config, event_tx, ready_tx));
    let (Ok(worker), Ok(hook_thread)) = (worker, hook_thread) else {
        eprintln!("스레드를 만들지 못했습니다.");
        return 1;
    };

    let thread_id = ready_rx.recv().unwrap_or(0);
    if thread_id == 0 {
        eprintln!("마우스 훅을 설치하지 못했습니다.");
        let _ = hook_thread.join();
        let _ = worker.join();
        return 1;
    }
    HOOK_THREAD.store(thread_id, Ordering::SeqCst);
    // SAFETY: 정적 함수 포인터를 등록한다.
    unsafe { SetConsoleCtrlHandler(Some(on_console_signal), TRUE) };

    println!(
        "Glide for Windows M1 실행 중 · 임계값 {} · 길게 누르기 {}ms · Ctrl+C로 종료",
        config.threshold, config.hold_ms
    );

    // 통계 출력 스레드. 종료 신호를 200ms마다 확인한다.
    if config.stats_sec > 0 {
        let interval = Duration::from_secs(config.stats_sec);
        thread::spawn(move || {
            let mut waited = Duration::ZERO;
            while !STOPPING.load(Ordering::SeqCst) {
                thread::sleep(Duration::from_millis(200));
                waited += Duration::from_millis(200);
                if waited >= interval {
                    waited = Duration::ZERO;
                    print_stats("통계");
                }
            }
        });
    }

    let _ = hook_thread.join();
    // 훅 스레드가 끝나면 채널 송신부가 사라져 워커도 끝난다.
    let _ = worker.join();
    STOPPING.store(true, Ordering::SeqCst);
    print_stats("종료");
    FINISHED.store(true, Ordering::SeqCst);
    0
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse(args: &[&str]) -> Result<Option<Config>, String> {
        parse_args(args.iter().map(|arg| arg.to_string()))
    }

    #[test]
    fn defaults_without_arguments() {
        let config = parse(&[]).unwrap().unwrap();
        assert_eq!(config.threshold, 22);
        assert_eq!(config.hold_ms, 400);
    }

    #[test]
    fn rejects_out_of_range_threshold() {
        assert!(parse(&["--threshold", "50"]).is_err());
        assert!(parse(&["--threshold"]).is_err());
        assert_eq!(
            parse(&["--threshold", "30"]).unwrap().unwrap().threshold,
            30
        );
    }

    #[test]
    fn help_returns_none() {
        assert!(parse(&["--help"]).unwrap().is_none());
    }
}
