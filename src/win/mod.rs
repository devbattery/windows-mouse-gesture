//! Win32 연결부: 스레드 시작·종료와 명령줄.
//!
//! 스레드 구성
//! - UI: 오버레이 창·트레이 아이콘(`ui.rs`)
//! - 훅: 저수준 마우스 훅, 판단만 하고 반환(`hook.rs`)
//! - 워커: 입력 재전송·동작 실행·설정 감시·예외 판정·설정 저장(`worker.rs`)
//! - 파이프: 설정 창 요청 처리(`pipe.rs`)
//! - 새 버전 확인: 하루 한 번 GitHub Releases에 최신 버전만 물음(`update.rs`, 설정으로 끌 수 있음)
//! - 로그: `--console`일 때만 출력(`log.rs`)

mod clock;
mod demo;
mod diagnostics;
mod hook;
mod input;
mod launch;
mod log;
mod overlay;
mod pipe;
mod settings_store;
mod shared;
mod tray;
mod ui;
mod update;
mod worker;

use std::ptr::null;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, SyncSender};
use std::thread;
use std::time::Duration;

use glide_core::ipc::{self, Request};
use glide_core::settings;
use serde_json::{Value, json};

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
use shared::{FULLSCREEN_PAUSED, HOOK_THREAD, RUNTIME_ENABLED, UI_ACCESS, UI_WINDOW, WM_UI_EXIT};

const USAGE: &str = "\
사용법: glide-core [옵션]

  --console        진단 로그와 훅 처리 시간 통계를 콘솔에 출력합니다
  --demo-overlay   견본 제스처(←)로 궤적·안내 표시를 그려 보고 종료합니다(진단용)
  --check-update   GitHub에서 새 버전을 한 번 확인해 결과를 보여 주고 끝납니다(진단용)
  -h, --help       이 도움말

설정은 %APPDATA%\\Glide\\settings.json에서 바꿉니다(저장하면 1초 안에 반영).
트레이 아이콘 메뉴에서 끄기·설정 열기·종료를 할 수 있습니다.";

struct Options {
    console: bool,
    demo_overlay: bool,
    check_update: bool,
}

fn parse_args(args: impl Iterator<Item = String>) -> Result<Option<Options>, String> {
    let mut options = Options {
        console: false,
        demo_overlay: false,
        check_update: false,
    };
    for arg in args {
        match arg.as_str() {
            "--console" => options.console = true,
            "--demo-overlay" => options.demo_overlay = true,
            "--check-update" => options.check_update = true,
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
    // 진단: 새 버전 확인만 하고 끝낸다. 실행 중인 Glide와 겹쳐도 되도록 단일 실행 확인보다 먼저 처리한다.
    if options.check_update {
        let (ok, report) = update::check_report();
        // SAFETY: 부모 콘솔에 붙어 보거나, 없으면 메시지 상자를 쓴다.
        if unsafe { AttachConsole(ATTACH_PARENT_PROCESS) } != 0 {
            println!("{report}");
        } else {
            message_box(&report);
        }
        return if ok { 0 } else { 1 };
    }
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

    // 설정 창과 통신할 파이프. 다른 프로그램이 같은 이름을 먼저 만들었으면(이름 선점) 실행하지 않는다.
    let pipe_server = match ipc::pipe_name().and_then(|name| pipe::create(&name)) {
        Ok(server) => server,
        Err(error) => {
            let message = format!(
                "설정 창과 통신할 통로를 만들지 못했습니다({error}).\n다른 프로그램이 같은 이름을 쓰고 있을 수 있어 실행을 멈춥니다."
            );
            if options.console {
                eprintln!("{message}");
            } else {
                message_box(&message);
            }
            return 1;
        }
    };
    UI_ACCESS.store(has_ui_access(), Ordering::SeqCst);
    // 실행 시간 기준점을 고정한다.
    std::sync::LazyLock::force(&diagnostics::STARTED);

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
            // 설정 창도 이 오류를 보여 주도록 기록한다(모르고 저장하면 파일이 기본값 기준으로 바뀐다).
            shared::set_settings_error(Some(message.clone()));
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
    update::start(ui_tx.clone());
    let pipe_events = event_tx.clone();
    let shutdown = event_tx.clone();
    let settings_path = store.path().display().to_string();
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
        let _ = shutdown.send(worker::Event::Shutdown);
        let _ = worker.join();
        message_box("마우스 훅을 설치하지 못했습니다.");
        return 1;
    }
    HOOK_THREAD.store(hook_id, Ordering::SeqCst);
    // 파이프 스레드는 연결을 기다리며 막혀 있으므로 기다리지 않는다(프로세스가 끝나면 함께 끝난다).
    let pipe_thread = thread::Builder::new()
        .name("glide-pipe".into())
        .spawn(move || {
            pipe::serve(
                pipe_server,
                |pid| ipc::is_sibling_process(pid, ipc::SETTINGS_EXE),
                |request| respond(request, &pipe_events, &settings_path),
            )
        });
    if pipe_thread.is_err() {
        log::warn(|| "[파이프] 스레드를 만들지 못해 설정 창과 연결할 수 없습니다".into());
    }
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
    // 파이프 스레드가 송신부를 들고 있어 채널이 닫히지 않으므로 워커에 종료를 알린다.
    let _ = shutdown.send(worker::Event::Shutdown);
    let _ = worker.join();
    if options.console {
        println!("[종료] {}", stats_line());
    }
    FINISHED.store(true, Ordering::SeqCst);
    0
}

/// 설정 창 요청 하나를 처리한다(파이프 스레드).
fn respond(request: Request, events: &SyncSender<worker::Event>, settings_path: &str) -> Value {
    match request {
        Request::GetStatus => {
            let snapshot = hook::HOOK_LATENCY.snapshot();
            let p99 = if snapshot.count() == 0 {
                None
            } else {
                snapshot.percentile_upper_bound(99.0)
            };
            ipc::success(json!({
                "version": env!("CARGO_PKG_VERSION"),
                "active": shared::gestures_active(),
                "runtimeEnabled": RUNTIME_ENABLED.load(Ordering::Relaxed),
                "fullscreenPaused": FULLSCREEN_PAUSED.load(Ordering::Relaxed),
                "uiAccess": UI_ACCESS.load(Ordering::Relaxed),
                "hookCalls": snapshot.count(),
                "hookP99Micros": p99,
                "hookOver10ms": snapshot.count() > 0 && p99.is_none(),
                "reinstalls": hook::REINSTALLS.load(Ordering::Relaxed),
                "settingsPath": settings_path,
                // 새 버전 확인 결과(지금보다 새 버전이 있을 때만 값이 있다)
                "update": {
                    "latest": update::available().map(|version| version.to_string()),
                },
            }))
        }
        Request::GetDiagnostics => ipc::success(diagnostics::collect()),
        Request::GetSettings => ipc::success(json!({
            "settings": shared::settings().to_value(),
            "error": shared::settings_error(),
        })),
        Request::SetSettings(value) => {
            // 설정 창의 검증과 별개로 코어가 다시 검증한다.
            let settings = match settings::validate(&value) {
                Ok(settings) => settings,
                Err(message) => return ipc::failure(message),
            };
            let (reply, result) = mpsc::sync_channel(1);
            let saved = events
                .send(worker::Event::Save {
                    settings: Box::new(settings),
                    reply,
                })
                .ok()
                .and_then(|()| result.recv_timeout(Duration::from_secs(5)).ok());
            match saved {
                Some(Ok(())) => ipc::success(json!({})),
                Some(Err(message)) => ipc::failure(message),
                // 워커가 다른 일로 늦어진 경우다. 요청은 대기열에 남아 곧 저장될 수 있으므로 실패라고 단정하지 않는다.
                None => ipc::failure(
                    "저장이 늦어지고 있습니다. 잠시 뒤 다시 열어 반영됐는지 확인해 주세요.",
                ),
            }
        }
    }
}

/// 이 프로세스 토큰에 uiAccess 표시가 있는지(서명·설치 위치 조건을 통과해 실행됐는지).
fn has_ui_access() -> bool {
    use windows_sys::Win32::Foundation::{CloseHandle, HANDLE};
    use windows_sys::Win32::Security::{GetTokenInformation, TOKEN_QUERY, TokenUIAccess};
    use windows_sys::Win32::System::Threading::{GetCurrentProcess, OpenProcessToken};

    // SAFETY: 자기 프로세스 토큰을 조회만 하고 닫는다.
    unsafe {
        let mut token: HANDLE = std::ptr::null_mut();
        if OpenProcessToken(GetCurrentProcess(), TOKEN_QUERY, &mut token) == 0 {
            return false;
        }
        let mut value: u32 = 0;
        let mut length = 0;
        let ok = GetTokenInformation(
            token,
            TokenUIAccess,
            (&mut value as *mut u32).cast(),
            size_of::<u32>() as u32,
            &mut length,
        );
        CloseHandle(token);
        ok != 0 && value != 0
    }
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
        assert!(parse(&["--check-update"]).unwrap().unwrap().check_update);
        assert!(parse(&["--help"]).unwrap().is_none());
        assert!(parse(&["--threshold", "30"]).is_err());
    }
}
