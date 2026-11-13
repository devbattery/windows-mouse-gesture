//! Win32 연결부: 스레드 시작·종료와 명령줄.
//!
//! 스레드 구성
//! - UI: 오버레이 창·트레이 아이콘(`ui.rs`)
//! - 훅: 저수준 마우스 훅, 판단만 하고 반환(`hook.rs`)
//! - 워커: 입력 재전송·동작 실행·설정 감시·예외 판정(`worker.rs`)
//! - 로그: `--console`일 때만 출력(`log.rs`)

mod clock;
mod demo;
mod hook;
mod input;
mod log;
mod overlay;
mod settings_store;
mod shared;
mod tray;
mod ui;
mod worker;

use std::ptr::null;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc;
use std::thread;
use std::time::Duration;

use windows_sys::Win32::Foundation::{ERROR_ALREADY_EXISTS, GetLastError, HWND, TRUE};
use windows_sys::Win32::System::Console::{
    ATTACH_PARENT_PROCESS, AllocConsole, AttachConsole, CTRL_CLOSE_EVENT, CTRL_LOGOFF_EVENT,
    CTRL_SHUTDOWN_EVENT, SetConsoleCtrlHandler,
};
use windows_sys::Win32::System::Threading::CreateMutexW;
use windows_sys::Win32::UI::HiDpi::{
    DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2, SetProcessDpiAwarenessContext,
};
use windows_sys::Win32::UI::WindowsAndMessaging::{
    MB_ICONINFORMATION, MB_OK, MessageBoxW, PostMessageW, PostThreadMessageW, WM_QUIT,
};
use windows_sys::core::BOOL;

use settings_store::{Change, Store};
use shared::{HOOK_THREAD, UI_WINDOW, WM_UI_EXIT};

const USAGE: &str = "\
사용법: glide-core [옵션]

  --console        진단 로그와 훅 처리 시간 통계를 콘솔에 출력합니다
  --demo-overlay   견본 제스처(←)로 궤적·안내 표시를 그려 보고 종료합니다(진단용)
  -h, --help       이 도움말

설정은 %APPDATA%\\Glide\\settings.json에서 바꿉니다(저장하면 1초 안에 반영).
트레이 아이콘 메뉴에서 끄기·설정 열기·종료를 할 수 있습니다.";

struct Options {
    console: bool,
    demo_overlay: bool,
}

fn parse_args(args: impl Iterator<Item = String>) -> Result<Option<Options>, String> {
    let mut options = Options {
        console: false,
        demo_overlay: false,
    };
    for arg in args {
        match arg.as_str() {
            "--console" => options.console = true,
            "--demo-overlay" => options.demo_overlay = true,
            "-h" | "--help" => return Ok(None),
            other => return Err(format!("알 수 없는 옵션입니다: {other}")),
        }
    }
    Ok(Some(options))
}

/// 정상 종료 경로(마지막 통계 출력)까지 마쳤는지
static FINISHED: AtomicBool = AtomicBool::new(false);

/// 콘솔 Ctrl+C·창 닫기 처리기. UI 스레드에 종료를 요청해 정상 종료 경로를 탄다.
unsafe extern "system" fn on_console_signal(signal: u32) -> BOOL {
    request_exit();
    // 창 닫기·로그오프·종료는 이 처리기가 돌아가는 즉시 Windows가 프로세스를 끝낸다.
    // 정상 종료를 잠시(최대 3초) 기다린다. Windows가 주는 유예는 약 5초다.
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

fn request_exit() {
    let window = UI_WINDOW.load(Ordering::SeqCst);
    if window != 0 {
        // SAFETY: UI 제어 창에 종료 메시지를 넣는다.
        unsafe { PostMessageW(window as HWND, WM_UI_EXIT, 0, 0) };
    }
}

fn wide(text: &str) -> Vec<u16> {
    text.encode_utf16().chain(std::iter::once(0)).collect()
}

/// 콘솔이 없을 때(기본 실행) 사용자에게 보여 줄 안내.
fn message_box(text: &str) {
    // SAFETY: 널 종료 문자열로 메시지 상자를 띄운다.
    unsafe {
        MessageBoxW(
            std::ptr::null_mut(),
            wide(text).as_ptr(),
            wide("Glide").as_ptr(),
            MB_OK | MB_ICONINFORMATION,
        )
    };
}

fn stats_line() -> String {
    format!(
        "{} · 재설치 {}회 · 버린 사건 {}",
        hook::HOOK_LATENCY.snapshot().summary(),
        hook::REINSTALLS.load(Ordering::Relaxed),
        hook::DROPPED_EVENTS.load(Ordering::Relaxed)
    )
}

/// 실행 진입점. 프로세스 종료 코드를 돌려준다.
pub fn run() -> i32 {
    let options = match parse_args(std::env::args().skip(1)) {
        Ok(Some(options)) => options,
        Ok(None) => {
            // 콘솔 없이 실행된 GUI 프로그램이라 도움말은 메시지 상자로도 보여 준다.
            // SAFETY: 부모 콘솔에 붙어 보거나, 없으면 메시지 상자를 쓴다.
            if unsafe { AttachConsole(ATTACH_PARENT_PROCESS) } != 0 {
                println!("{USAGE}");
            } else {
                message_box(USAGE);
            }
            return 0;
        }
        Err(message) => {
            message_box(&format!("{message}\n\n{USAGE}"));
            return 2;
        }
    };
    if options.console {
        // SAFETY: 부모(터미널) 콘솔에 붙고, 없으면 새 콘솔을 만든다.
        unsafe {
            if AttachConsole(ATTACH_PARENT_PROCESS) == 0 {
                AllocConsole();
            }
        }
        log::start();
    }

    // 같은 사용자 세션에서 두 번 실행되면 우클릭을 이중으로 가로채므로 막는다.
    // 핸들은 프로세스가 끝날 때까지 유지해야 하므로 닫지 않는다.
    let name = wide("Local\\GlideWindowsGesture.Core");
    // SAFETY: 널 보안 속성과 널 종료 UTF-16 이름을 넘긴다.
    let mutex = unsafe { CreateMutexW(null(), 0, name.as_ptr()) };
    if mutex.is_null() || unsafe { GetLastError() } == ERROR_ALREADY_EXISTS {
        if options.console {
            eprintln!("이미 실행 중입니다. 트레이 아이콘을 확인해 주세요.");
        } else {
            message_box("이미 실행 중입니다. 알림 영역(트레이)의 아이콘을 확인해 주세요.");
        }
        return 1;
    }

    // 훅 좌표를 물리 픽셀로 받고 모니터별 DPI로 임계값·오버레이를 맞추기 위해 Per-Monitor v2로 동작한다.
    // SAFETY: 프로세스 시작 직후 창을 만들기 전에 한 번 호출한다. 실패해도(구버전 Windows) 계속 실행한다.
    unsafe { SetProcessDpiAwarenessContext(DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2) };
    clock::init();

    let mut store = Store::new();
    let startup_notice = match store.load_or_create() {
        Change::Loaded(settings) => {
            shared::publish(settings);
            None
        }
        Change::Invalid(message) => {
            eprintln!("[설정] {message} (기본 설정으로 실행합니다)");
            Some(format!("{message}\n기본 설정으로 실행합니다."))
        }
        Change::None => None,
    };
    let settings_file = store.path().to_path_buf();
    let settings_folder = store.folder();

    // 훅 → 워커(동작), 훅·워커 → UI(표시)
    let (event_tx, event_rx) = mpsc::sync_channel(1024);
    let (ui_tx, ui_rx) = mpsc::sync_channel(4096);

    let (ui_ready_tx, ui_ready_rx) = mpsc::channel();
    let ui_thread = thread::Builder::new()
        .name("glide-ui".into())
        .spawn(move || {
            ui::run(
                ui_rx,
                settings_file,
                settings_folder,
                ui_ready_tx,
                startup_notice,
            )
        });
    let Ok(ui_thread) = ui_thread else {
        message_box("UI 스레드를 만들지 못했습니다.");
        return 1;
    };
    if ui_ready_rx.recv().unwrap_or(0) == 0 {
        let _ = ui_thread.join();
        message_box("트레이 창을 만들지 못했습니다.");
        return 1;
    }

    let worker_ui = ui_tx.clone();
    let demo_ui = options.demo_overlay.then(|| ui_tx.clone());
    let worker = thread::Builder::new()
        .name("glide-worker".into())
        .spawn(move || worker::run(event_rx, worker_ui, store));
    let (hook_ready_tx, hook_ready_rx) = mpsc::channel();
    let hook_thread = thread::Builder::new()
        .name("glide-hook".into())
        .spawn(move || hook::run(event_tx, ui_tx, hook_ready_tx));
    let (Ok(worker), Ok(hook_thread)) = (worker, hook_thread) else {
        request_exit();
        let _ = ui_thread.join();
        message_box("스레드를 만들지 못했습니다.");
        return 1;
    };
    let hook_id = hook_ready_rx.recv().unwrap_or(0);
    if hook_id == 0 {
        request_exit();
        let _ = ui_thread.join();
        let _ = hook_thread.join();
        let _ = worker.join();
        message_box("마우스 훅을 설치하지 못했습니다.");
        return 1;
    }
    HOOK_THREAD.store(hook_id, Ordering::SeqCst);
    if let Some(demo_ui) = demo_ui {
        thread::spawn(move || demo::run(demo_ui));
    }

    if options.console {
        // SAFETY: 정적 함수 포인터를 등록한다.
        unsafe { SetConsoleCtrlHandler(Some(on_console_signal), TRUE) };
        log::info(|| {
            format!(
                "Glide for Windows 실행 중 · 설정 {} · Ctrl+C 또는 트레이 메뉴로 종료",
                store_path_hint()
            )
        });
        // 30초마다 훅 처리 시간 통계
        thread::spawn(|| {
            let mut waited = Duration::ZERO;
            while !FINISHED.load(Ordering::SeqCst) {
                thread::sleep(Duration::from_millis(200));
                waited += Duration::from_millis(200);
                if waited >= Duration::from_secs(30) {
                    waited = Duration::ZERO;
                    log::info(|| format!("[통계] {}", stats_line()));
                }
            }
        });
    }

    // 트레이 "종료"나 Ctrl+C로 UI 스레드가 끝나면 훅을 멈추고 워커를 기다린다.
    let _ = ui_thread.join();
    // SAFETY: 훅 스레드 메시지 루프를 끝낸다.
    unsafe { PostThreadMessageW(hook_id, WM_QUIT, 0, 0) };
    let _ = hook_thread.join();
    // 훅 스레드가 끝나면 채널 송신부가 사라져 워커도 끝난다.
    let _ = worker.join();
    if options.console {
        println!("[종료] {}", stats_line());
    }
    FINISHED.store(true, Ordering::SeqCst);
    0
}

fn store_path_hint() -> String {
    std::env::var("APPDATA").map_or_else(
        |_| "settings.json".into(),
        |base| format!("{base}\\Glide\\settings.json"),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse(args: &[&str]) -> Result<Option<Options>, String> {
        parse_args(args.iter().map(|arg| arg.to_string()))
    }

    #[test]
    fn console_flag_and_help() {
        assert!(!parse(&[]).unwrap().unwrap().console);
        assert!(parse(&["--console"]).unwrap().unwrap().console);
        assert!(parse(&["--help"]).unwrap().is_none());
        assert!(parse(&["--threshold", "30"]).is_err());
    }
}
