//! 워커 스레드. 훅이 넘긴 일과 주기 작업을 처리한다. 여기서는 시간이 걸려도 마우스 입력이
//! 끊기지 않는다.
//!
//! - 우클릭 재전송, 제스처 동작 실행
//! - 처음 보는 프로세스의 예외 앱 판정
//! - 1초마다 settings.json 변경과 전체 화면(게임·프레젠테이션) 상태 확인

use std::sync::atomic::Ordering;
use std::sync::mpsc::{Receiver, RecvTimeoutError, SyncSender};
use std::time::{Duration, Instant};

use glide_core::actions::Effect;
use glide_core::gesture::Sequence;
use glide_core::history::{Entry, Kind};
use glide_core::ipc;
use glide_core::machine::Ending;
use glide_core::settings::Settings;
use windows_sys::Win32::UI::Shell::{
    QUNS_PRESENTATION_MODE, QUNS_RUNNING_D3D_FULL_SCREEN, SHQueryUserNotificationState,
};
use windows_sys::Win32::UI::WindowsAndMessaging::{
    GA_ROOT, GetAncestor, GetClassNameW, GetForegroundWindow, IsWindow, IsZoomed, PostMessageW,
    PostThreadMessageW, SW_MAXIMIZE, SW_MINIMIZE, SW_RESTORE, SetForegroundWindow, ShowWindowAsync,
    WM_CLOSE,
};

use super::settings_store::{Change, Store};
use super::shared::{
    self, FULLSCREEN_PAUSED, HOOK_THREAD, WM_HOOK_CLEAR_EXCLUSIONS, WM_HOOK_EXCLUSION,
};
use super::ui::{self, UiEvent};
use super::{input, log};

/// 다시 보낼 우클릭의 종류.
#[derive(Clone, Copy, Debug)]
pub enum Injection {
    /// 움직이지 않고 뗌 → 누름+뗌
    Click,
    /// 길게 누름·예외 앱 → 누름만(이후 실제 드래그·뗌은 통과)
    Press,
    /// 다시 보낸 누름보다 실제 뗌이 먼저 옴 → 뗌을 뒤에 보냄
    Release,
}

/// 훅·파이프 스레드 → 워커 스레드 메시지. 훅이 보내는 값은 할당 없이 만든다.
/// `target`은 우클릭을 시작한 지점의 최상위 창 핸들 값이다.
#[derive(Debug)]
pub enum Event {
    Inject {
        kind: Injection,
        target: isize,
    },
    Finished {
        sequence: Sequence,
        invalid: bool,
        target: isize,
        pid: u32,
    },
    /// 처음 보는 프로세스가 예외 앱인지 판정해 달라
    Resolve {
        pid: u32,
    },
    /// 우클릭 하나가 제스처 실행 말고 다른 방식으로 끝남(최근 기록용)
    Outcome {
        ending: Ending,
        pid: u32,
    },
    /// 설정 창이 보낸(코어가 검증한) 설정을 저장하고 반영해 달라. 결과를 `reply`로 돌려준다.
    Save {
        settings: Box<Settings>,
        reply: SyncSender<Result<(), String>>,
    },
    /// 종료. 파이프 스레드가 송신부를 끝까지 들고 있어 채널이 닫히지 않으므로 따로 알린다.
    Shutdown,
}

const PERIOD: Duration = Duration::from_secs(1);

/// `Shutdown`을 받거나 채널이 닫힐 때까지 사건과 주기 작업을 처리한다.
pub fn run(events: Receiver<Event>, ui: SyncSender<UiEvent>, mut store: Store) {
    let mut last_periodic = Instant::now();
    loop {
        match events.recv_timeout(PERIOD) {
            Ok(Event::Shutdown) => break,
            Ok(Event::Save { settings, reply }) => {
                let result = save(&mut store, *settings, &ui);
                let _ = reply.try_send(result);
            }
            Ok(event) => handle(event, &ui),
            Err(RecvTimeoutError::Timeout) => {}
            Err(RecvTimeoutError::Disconnected) => break,
        }
        if last_periodic.elapsed() >= PERIOD {
            last_periodic = Instant::now();
            let before = shared::gestures_active();
            check_settings(&mut store, &ui);
            check_fullscreen();
            // 사용 여부가 바뀌었으면 트레이 도움말을 갱신한다.
            if shared::gestures_active() != before && ui.try_send(UiEvent::StateChanged).is_ok() {
                ui::wake();
            }
        }
    }
}

fn handle(event: Event, ui: &SyncSender<UiEvent>) {
    match event {
        Event::Inject { kind, target } => {
            // 입력을 먼저 보내고 로그는 그 뒤에 남긴다(프로세스 이름 조회가 재전송을 늦추지 않게).
            let sent = match kind {
                Injection::Click => input::right_click(),
                Injection::Press => input::right_down(),
                Injection::Release => input::right_up(),
            };
            let what = match kind {
                Injection::Click => "단순 우클릭으로 다시 보냄",
                Injection::Press => "원래 우클릭으로 통과",
                Injection::Release => "뗌 순서를 보정해 다시 보냄",
            };
            if sent {
                log::info(|| format!("[우클릭] {what} · {}", app_name(target)));
            } else {
                log::warn(|| {
                    format!(
                        "[경고] 우클릭을 다시 보내지 못했습니다({what}) · {}",
                        app_name(target)
                    )
                });
            }
        }
        Event::Resolve { pid } => {
            let name = process_name(pid);
            let excluded = name.as_deref().is_some_and(is_excluded);
            if excluded {
                log::info(|| {
                    format!(
                        "[예외] {} 에서는 제스처를 쓰지 않습니다",
                        name.as_deref().unwrap_or("")
                    )
                });
            }
            post_hook(WM_HOOK_EXCLUSION, pid as usize, excluded as isize);
        }
        Event::Finished {
            sequence,
            invalid,
            target,
            pid,
        } => {
            let (kind, detail) = execute(sequence, invalid, target, pid, ui);
            remember(pid, kind, detail);
        }
        Event::Outcome { ending, pid } => remember(pid, ending.into(), None),
        // run에서 처리한다(저장소가 필요하거나 루프를 끝낸다).
        Event::Save { .. } | Event::Shutdown => {}
    }
}

/// 최근 기록에 한 건 남긴다. 앱은 실행 파일 이름만 남긴다(알 수 없으면 빈 문자열).
fn remember(pid: u32, kind: Kind, detail: Option<String>) {
    let at = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |elapsed| elapsed.as_millis() as u64);
    shared::record(Entry {
        at,
        app: process_name(pid).unwrap_or_default(),
        kind,
        detail,
    });
}

/// 제스처 하나를 실행하고, 최근 기록에 남길 결과(종류, 동작 ID·방향·실패 이유)를 돌려준다.
fn execute(
    sequence: Sequence,
    invalid: bool,
    target: isize,
    pid: u32,
    ui: &SyncSender<UiEvent>,
) -> (Kind, Option<String>) {
    let app = app_name(target);
    let directions = (!sequence.is_empty()).then(|| sequence.as_str().to_string());
    if invalid || sequence.is_empty() {
        log::info(|| {
            format!(
                "[결과] 실행할 제스처가 아닙니다({}) · {app}",
                sequence.arrows()
            )
        });
        return (Kind::NotGesture, directions);
    }
    let settings = shared::settings();
    let Some(rule) = settings
        .rules
        .iter()
        .find(|rule| rule.gesture == sequence.as_str())
    else {
        log::info(|| format!("[결과] 등록되지 않은 제스처 {} · {app}", sequence.arrows()));
        return (Kind::Unmatched, directions);
    };
    // 낙관적 시작 뒤 판정이 늦게 끝난 예외 앱: 실행하지 않는다.
    // 이미 제스처로 삼켜져 앱에 우클릭이 전달되지 않았으므로 "원래 우클릭"(Excluded)이 아니라 실행 못 함으로 남긴다.
    if process_name(pid).is_some_and(|name| is_excluded(&name)) {
        let message = "제스처를 쓰지 않는 앱입니다";
        notify(ui, sequence, message);
        return (Kind::Failed, Some(message.into()));
    }
    let effect = rule.effect();
    let window_effect = matches!(
        effect,
        Effect::MinimizeWindow | Effect::ToggleMaximize | Effect::CloseWindow
    );
    if window_effect && is_shell_window(target) {
        let message = "바탕 화면·작업 표시줄에는 실행하지 않습니다";
        notify(ui, sequence, message);
        return (Kind::Failed, Some(message.into()));
    }
    let result: Result<(), &str> = match &effect {
        Effect::Keys(shortcut) => {
            // 삼킨 우클릭은 창을 활성화하지 않는다. 다른 창에 키가 들어가지 않도록 대상 창을 확인한다.
            if !ensure_foreground(target) {
                Err("제스처를 시작한 창을 활성화하지 못했습니다")
            } else if input::shortcut(shortcut) {
                Ok(())
            } else {
                Err("키 입력을 보내지 못했습니다")
            }
        }
        // 창 명령은 비동기로 보내 응답 없는 앱 때문에 워커가 멈추지 않게 한다.
        Effect::MinimizeWindow => show_window(target, SW_MINIMIZE),
        Effect::ToggleMaximize => {
            // SAFETY: 창 스타일 조회다. 창이 사라졌으면 0을 돌려준다.
            let zoomed = unsafe { IsZoomed(target as _) } != 0;
            show_window(target, if zoomed { SW_RESTORE } else { SW_MAXIMIZE })
        }
        Effect::CloseWindow => {
            // SAFETY: 창 핸들 값으로 닫기 요청을 넣는다. 창이 사라졌으면 실패만 한다.
            if target != 0 && unsafe { PostMessageW(target as _, WM_CLOSE, 0, 0) } != 0 {
                Ok(())
            } else {
                Err("창을 닫지 못했습니다")
            }
        }
        Effect::MediaPlayPause => {
            if input::media_play_pause() {
                Ok(())
            } else {
                Err("키 입력을 보내지 못했습니다")
            }
        }
        Effect::Unsupported => Err("Windows에서 지원하지 않는 동작입니다"),
    };
    match result {
        Ok(()) => {
            log::info(|| format!("[결과] {} → {} · {app}", sequence.arrows(), rule.action));
            (Kind::Executed, Some(rule.action.clone()))
        }
        Err(message) => {
            notify(ui, sequence, message);
            (Kind::Failed, Some(message.into()))
        }
    }
}

/// 실행 실패를 HUD와 로그로 알린다.
fn notify(ui: &SyncSender<UiEvent>, sequence: Sequence, message: &'static str) {
    log::info(|| format!("[결과] {} · {message}", sequence.arrows()));
    if ui.try_send(UiEvent::Notice { sequence, message }).is_ok() {
        ui::wake();
    }
}

fn show_window(target: isize, command: i32) -> Result<(), &'static str> {
    // SAFETY: 창 핸들 값으로 비동기 창 명령을 넣는다.
    if target != 0 && unsafe { IsWindow(target as _) } != 0 {
        unsafe { ShowWindowAsync(target as _, command) };
        Ok(())
    } else {
        Err("대상 창을 찾지 못했습니다")
    }
}

/// 바탕 화면(Progman·WorkerW)과 작업 표시줄. 닫기 요청을 보내면 Windows 종료 창이 뜨므로 막는다.
fn is_shell_window(window: isize) -> bool {
    if window == 0 {
        return true;
    }
    let mut buffer = [0u16; 64];
    // SAFETY: 창 클래스 이름 조회다(창에 메시지를 보내지 않음).
    let length = unsafe { GetClassNameW(window as _, buffer.as_mut_ptr(), buffer.len() as i32) };
    let class = String::from_utf16_lossy(&buffer[..length.max(0) as usize]);
    matches!(
        class.as_str(),
        "Progman" | "WorkerW" | "Shell_TrayWnd" | "Shell_SecondaryTrayWnd"
    )
}

fn post_hook(message: u32, wparam: usize, lparam: isize) {
    let thread = HOOK_THREAD.load(Ordering::SeqCst);
    if thread != 0 {
        // SAFETY: 훅 스레드 ID로 스레드 메시지를 넣는다.
        unsafe { PostThreadMessageW(thread, message, wparam, lparam) };
    }
}

/// settings.json이 바뀌었으면 다시 읽어 반영한다.
fn check_settings(store: &mut Store, ui: &SyncSender<UiEvent>) {
    match store.poll() {
        Change::None => {}
        Change::Loaded(settings) => {
            log::info(|| format!("[설정] 다시 읽음 · 제스처 {}개", settings.rules.len()));
            apply(settings);
        }
        Change::Invalid(message) => {
            log::warn(|| format!("[설정] {message} (이전 설정을 계속 씁니다)"));
            shared::set_settings_error(Some(message.clone()));
            if ui.try_send(UiEvent::SettingsError { message }).is_ok() {
                ui::wake();
            }
        }
    }
}

/// 검증된 설정을 모든 스레드에 반영한다.
fn apply(settings: Settings) {
    shared::set_settings_error(None);
    shared::publish(settings);
    // 예외 목록이 바뀌었을 수 있으므로 훅의 판정 캐시를 비운다.
    post_hook(WM_HOOK_CLEAR_EXCLUSIONS, 0, 0);
}

/// 설정 창이나 트레이 메뉴에서 받은 설정을 파일에 저장하고 바로 반영한다.
fn save(store: &mut Store, settings: Settings, ui: &SyncSender<UiEvent>) -> Result<(), String> {
    store
        .save(&settings)
        .map_err(|error| format!("설정 파일을 저장하지 못했습니다: {error}"))?;
    log::info(|| {
        format!(
            "[설정] 저장 · 제스처 {}개 · 예외 앱 {}개",
            settings.rules.len(),
            settings.excluded_processes.len()
        )
    });
    let before = shared::gestures_active();
    apply(settings);
    if shared::gestures_active() != before && ui.try_send(UiEvent::StateChanged).is_ok() {
        ui::wake();
    }
    Ok(())
}

/// 제스처를 쓰지 않는 앱인지. 설정 창은 제스처를 녹화하는 곳이라 항상 제외한다.
fn is_excluded(process_name: &str) -> bool {
    process_name.eq_ignore_ascii_case(ipc::SETTINGS_EXE)
        || shared::settings().is_excluded(process_name)
}

/// 전체 화면 D3D 게임·프레젠테이션 모드면 제스처를 잠시 멈춘다.
/// 테두리 없는 전체 화면(QUNS_BUSY)은 브라우저 F11과 구분되지 않아 멈추지 않는다. 그런 게임은 예외 목록에 넣는다.
fn check_fullscreen() {
    let pause = shared::settings().auto_pause_fullscreen && {
        let mut state = 0;
        // SAFETY: 출력 포인터 하나를 넘긴다.
        let result = unsafe { SHQueryUserNotificationState(&mut state) };
        result >= 0 && (state == QUNS_RUNNING_D3D_FULL_SCREEN || state == QUNS_PRESENTATION_MODE)
    };
    if FULLSCREEN_PAUSED.swap(pause, Ordering::SeqCst) != pause {
        log::info(|| {
            format!(
                "[전체 화면] 제스처 {}",
                if pause {
                    "일시 중지"
                } else {
                    "다시 사용"
                }
            )
        });
    }
}

/// 창을 소유한 프로세스의 실행 파일 이름(로그용). 알 수 없으면 "알 수 없는 앱".
fn app_name(window: isize) -> String {
    let mut pid = 0;
    if window != 0 {
        // SAFETY: 창 핸들 값과 출력 포인터를 넘긴다. 창이 사라졌으면 0을 돌려준다.
        unsafe {
            windows_sys::Win32::UI::WindowsAndMessaging::GetWindowThreadProcessId(
                window as _,
                &mut pid,
            )
        };
    }
    process_name(pid).unwrap_or_else(|| {
        if pid == 0 {
            "알 수 없는 앱".into()
        } else {
            format!("PID {pid}")
        }
    })
}

/// 프로세스의 실행 파일 이름(소문자 아님, 예: notepad.exe). 창 제목은 개인 정보가 담길 수 있어 읽지 않는다.
/// 관리자 권한 프로세스처럼 열 수 없으면 None.
fn process_name(pid: u32) -> Option<String> {
    let path = ipc::process_path(pid)?;
    Some(path.file_name()?.to_string_lossy().into_owned())
}

fn foreground_root() -> isize {
    // SAFETY: 조회 함수들이다.
    unsafe {
        let window = GetForegroundWindow();
        if window.is_null() {
            0
        } else {
            GetAncestor(window, GA_ROOT) as isize
        }
    }
}

/// 대상 창이 전면 창이면 true. 아니면 활성화를 한 번 시도하고 결과를 돌려준다.
fn ensure_foreground(target: isize) -> bool {
    if target == 0 {
        return false;
    }
    if foreground_root() == target {
        return true;
    }
    // SAFETY: 창 핸들 값을 그대로 넘긴다. 창이 사라졌으면 실패만 한다.
    unsafe { SetForegroundWindow(target as _) };
    foreground_root() == target
}
