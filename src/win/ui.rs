//! UI 스레드: 오버레이 창과 트레이 아이콘.
//!
//! 숨긴 제어 창 하나가 모든 UI 메시지를 받는다. 훅·워커는 채널에 사건을 넣고 [`wake`]로
//! 이 창을 깨운다. 스레드 메시지 대신 창 메시지를 쓰는 이유는, 트레이 메뉴 같은 모달 루프가
//! 도는 동안 스레드 메시지는 버려지지만 창 메시지는 창 프로시저로 전달되기 때문이다.

use std::cell::RefCell;
use std::path::PathBuf;
use std::ptr::{null, null_mut};
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::sync::mpsc::{Receiver, Sender};

use glide_core::actions::{Hud, Tone, gesture_hud};
use glide_core::gesture::Sequence;
use glide_core::ipc;
use windows_sys::Win32::Foundation::{HWND, LPARAM, LRESULT, POINT, WPARAM};
use windows_sys::Win32::System::Com::{
    COINIT_APARTMENTTHREADED, COINIT_DISABLE_OLE1DDE, CoInitializeEx, CoUninitialize,
};
use windows_sys::Win32::System::LibraryLoader::GetModuleHandleW;
use windows_sys::Win32::System::RemoteDesktop::{
    NOTIFY_FOR_THIS_SESSION, WTSRegisterSessionNotification, WTSUnRegisterSessionNotification,
};
use windows_sys::Win32::UI::Shell::{NIN_SELECT, NINF_KEY};
use windows_sys::Win32::UI::WindowsAndMessaging::{
    AppendMenuW, CreatePopupMenu, CreateWindowExW, DefWindowProcW, DestroyMenu, DestroyWindow,
    DispatchMessageW, GetCursorPos, GetMessageW, KillTimer, MF_CHECKED, MF_GRAYED, MF_SEPARATOR,
    MF_STRING, MSG, PBT_APMRESUMEAUTOMATIC, PBT_APMRESUMESUSPEND, PostMessageW, PostQuitMessage,
    PostThreadMessageW, RegisterClassExW, RegisterWindowMessageW, SetForegroundWindow, SetTimer,
    TPM_NONOTIFY, TPM_RETURNCMD, TPM_RIGHTBUTTON, TrackPopupMenu, WM_CONTEXTMENU, WM_DISPLAYCHANGE,
    WM_NULL, WM_POWERBROADCAST, WM_TIMER, WM_WTSSESSION_CHANGE, WNDCLASSEXW, WS_OVERLAPPED,
};

use super::overlay::Overlay;
use super::shared::{
    self, FULLSCREEN_PAUSED, HOOK_THREAD, RUNTIME_ENABLED, ResetReason, SETTINGS_ENABLED,
    UI_ACCESS, UI_WINDOW, WM_HOOK_RESET, WM_UI_EXIT, WM_UI_TRAY, WM_UI_WAKE,
};
use super::tray::Tray;
use super::{hook, launch, log};

/// 훅·워커 → UI 스레드 사건.
#[derive(Clone, Debug)]
pub enum UiEvent {
    /// 제스처 시작(오른쪽 버튼을 누른 화면 좌표)
    Begin {
        x: i32,
        y: i32,
    },
    /// 궤적 점(화면 좌표, 물리 픽셀)
    Point {
        x: i32,
        y: i32,
    },
    Changed {
        sequence: Sequence,
        invalid: bool,
    },
    Finished {
        sequence: Sequence,
        invalid: bool,
    },
    Cancelled,
    /// 실행 실패 등 결과 안내
    Notice {
        sequence: Sequence,
        message: &'static str,
    },
    /// 설정 파일 오류(트레이 알림)
    SettingsError {
        message: String,
    },
    /// 사용 여부·전체 화면 일시 중지가 바뀜(트레이 도움말 갱신)
    StateChanged,
}

/// 실행된 제스처 안내 표시 시간(확장과 같음)
const RESULT_MS: u32 = 750;
/// 실패 안내 표시 시간(확장과 같음)
const NOTICE_MS: u32 = 2_000;
const HIDE_TIMER: usize = 1;

const MENU_TOGGLE: usize = 1;
const MENU_OPEN_SETTINGS: usize = 2;
const MENU_OPEN_FOLDER: usize = 3;
const MENU_EXIT: usize = 4;

static WAKE_PENDING: AtomicBool = AtomicBool::new(false);
/// Explorer 재시작 알림 메시지 번호
static TASKBAR_CREATED: AtomicU32 = AtomicU32::new(0);

/// UI 스레드를 깨운다. 이미 깨우기 메시지가 대기 중이면 다시 보내지 않는다(여러 사건을 한 번에 처리).
pub fn wake() {
    if WAKE_PENDING.swap(true, Ordering::AcqRel) {
        return;
    }
    let window = UI_WINDOW.load(Ordering::Acquire);
    // SAFETY: UI 제어 창에 메시지를 넣는다. 창이 없으면 다음 깨우기를 위해 표시를 되돌린다.
    if window == 0 || unsafe { PostMessageW(window as HWND, WM_UI_WAKE, 0, 0) } == 0 {
        WAKE_PENDING.store(false, Ordering::Release);
    }
}

struct Ui {
    window: HWND,
    events: Receiver<UiEvent>,
    overlay: Option<Overlay>,
    tray: Tray,
    settings_file: PathBuf,
    settings_folder: PathBuf,
    /// 손을 뗀 뒤 결과를 보여 주는 중(실패 안내를 덧붙일 수 있음)
    showing_result: bool,
}

thread_local! {
    static UI: RefCell<Option<Ui>> = const { RefCell::new(None) };
}

fn with_ui<R>(f: impl FnOnce(&mut Ui) -> R) -> Option<R> {
    UI.with(|cell| match cell.try_borrow_mut() {
        Ok(mut guard) => guard.as_mut().map(f),
        Err(_) => None,
    })
}

fn wide(text: &str) -> Vec<u16> {
    text.encode_utf16().chain(std::iter::once(0)).collect()
}

/// UI 스레드 본체. 준비되면 `ready`로 제어 창 핸들을(실패하면 0을) 보내고, 종료 요청까지 돈다.
pub fn run(
    events: Receiver<UiEvent>,
    settings_file: PathBuf,
    settings_folder: PathBuf,
    ready: Sender<isize>,
    startup_notice: Option<String>,
) {
    // 탐색기를 통한 실행(launch.rs, 셸 COM 객체)을 위해 단일 스레드 아파트로 COM을 초기화한다.
    // SAFETY: 이 스레드에서 한 번 초기화하고 끝날 때 해제한다.
    let com = unsafe {
        CoInitializeEx(
            null(),
            (COINIT_APARTMENTTHREADED | COINIT_DISABLE_OLE1DDE) as u32,
        )
    } >= 0;
    let class_name = wide("GlideControl");
    // SAFETY: 정적 창 프로시저로 숨긴 제어 창을 만든다. 이 창은 화면에 보이지 않는다.
    let window = unsafe {
        let instance = GetModuleHandleW(null());
        let class = WNDCLASSEXW {
            cbSize: size_of::<WNDCLASSEXW>() as u32,
            lpfnWndProc: Some(control_proc),
            hInstance: instance,
            lpszClassName: class_name.as_ptr(),
            ..std::mem::zeroed()
        };
        RegisterClassExW(&class);
        CreateWindowExW(
            0,
            class_name.as_ptr(),
            wide("Glide").as_ptr(),
            WS_OVERLAPPED,
            0,
            0,
            0,
            0,
            null_mut(),
            null_mut(),
            instance,
            null(),
        )
    };
    if window.is_null() {
        let _ = ready.send(0);
        return;
    }
    // 화면 잠금·해제, 원격 연결·해제 알림(WM_WTSSESSION_CHANGE)을 받는다. 실패해도 계속 실행한다
    // (제스처 중 보안 데스크톱 전환은 훅 스레드가 따로 확인한다).
    // SAFETY: 이 스레드가 만든 창을 등록하고, 끝날 때 해제한다.
    let session_notifications =
        unsafe { WTSRegisterSessionNotification(window, NOTIFY_FOR_THIS_SESSION) } != 0;
    let overlay = match Overlay::new() {
        Ok(overlay) => Some(overlay),
        Err(error) => {
            // 오버레이 없이도 제스처 실행은 된다.
            log::warn(|| format!("[오버레이] 만들지 못했습니다: {error}"));
            None
        }
    };
    let mut tray = Tray::new(window);
    tray.add(&tip());
    if let Some(message) = &startup_notice {
        tray.warn("Glide 설정 파일 오류", message);
    }
    // SAFETY: 널 종료 문자열로 등록된 메시지 번호를 얻는다.
    TASKBAR_CREATED.store(
        unsafe { RegisterWindowMessageW(wide("TaskbarCreated").as_ptr()) },
        Ordering::SeqCst,
    );
    UI.with(|cell| {
        *cell.borrow_mut() = Some(Ui {
            window,
            events,
            overlay,
            tray,
            settings_file,
            settings_folder,
            showing_result: false,
        })
    });
    UI_WINDOW.store(window as isize, Ordering::Release);
    let _ = ready.send(window as isize);

    // SAFETY: MSG는 0으로 초기화해도 유효한 C 구조체다.
    let mut message: MSG = unsafe { std::mem::zeroed() };
    while unsafe { GetMessageW(&mut message, null_mut(), 0, 0) } > 0 {
        // SAFETY: 받은 메시지를 그대로 넘긴다.
        unsafe { DispatchMessageW(&message) };
    }

    UI_WINDOW.store(0, Ordering::Release);
    // 트레이 아이콘 제거와 오버레이 해제(Drop)
    UI.with(|cell| cell.borrow_mut().take());
    // SAFETY: 이 스레드가 만든 창을 닫고 COM을 해제한다.
    unsafe {
        if session_notifications {
            WTSUnRegisterSessionNotification(window);
        }
        DestroyWindow(window);
        if com {
            CoUninitialize();
        }
    }
}

unsafe extern "system" fn control_proc(
    window: HWND,
    message: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    match message {
        WM_UI_WAKE => {
            // 표시를 먼저 내려야 처리 중에 들어온 사건이 다음 깨우기를 보낸다.
            WAKE_PENDING.store(false, Ordering::Release);
            with_ui(Ui::drain);
            0
        }
        WM_TIMER if wparam == HIDE_TIMER => {
            // SAFETY: 이 창의 타이머를 끈다.
            unsafe { KillTimer(window, HIDE_TIMER) };
            with_ui(Ui::hide_overlay);
            0
        }
        WM_UI_TRAY => {
            // NOTIFYICON_VERSION_4: lParam 하위 16비트가 사건 종류
            let event = (lparam as u32) & 0xFFFF;
            // 오른쪽 클릭(WM_CONTEXTMENU), 왼쪽 클릭(NIN_SELECT), 키보드 선택(NIN_KEYSELECT = NIN_SELECT | NINF_KEY)
            if event == WM_CONTEXTMENU || event == NIN_SELECT || event == (NIN_SELECT | NINF_KEY) {
                show_menu(window);
            }
            0
        }
        WM_UI_EXIT => {
            // SAFETY: 이 스레드의 메시지 루프를 끝낸다.
            unsafe { PostQuitMessage(0) };
            0
        }
        // 잠금·해제·원격 연결 등: 뗌을 놓쳤을 수 있으므로 추적을 초기화한다.
        WM_WTSSESSION_CHANGE => {
            reset_tracking(ResetReason::Session);
            0
        }
        WM_POWERBROADCAST => {
            if matches!(wparam as u32, PBT_APMRESUMEAUTOMATIC | PBT_APMRESUMESUSPEND) {
                reset_tracking(ResetReason::Resume);
            }
            // 전원 알림에는 TRUE를 돌려준다.
            1
        }
        WM_DISPLAYCHANGE => {
            // 모니터 배치가 바뀌면 그리던 표면의 좌표가 맞지 않으므로 진행 중인 제스처를 취소한다.
            reset_tracking(ResetReason::Display);
            // SAFETY: 기본 처리기로도 넘긴다.
            unsafe { DefWindowProcW(window, message, wparam, lparam) }
        }
        _ if message != 0 && message == TASKBAR_CREATED.load(Ordering::Relaxed) => {
            // Explorer가 다시 시작됨: 트레이 아이콘을 다시 등록한다.
            with_ui(|ui| ui.tray.add(&tip()));
            0
        }
        // SAFETY: 처리하지 않은 메시지를 기본 처리기로 넘긴다.
        _ => unsafe { DefWindowProcW(window, message, wparam, lparam) },
    }
}

impl Ui {
    /// 쌓인 사건을 모두 처리한 뒤 한 번 그린다.
    fn drain(&mut self) {
        while let Ok(event) = self.events.try_recv() {
            self.on_event(event);
        }
        if let Some(overlay) = &mut self.overlay {
            overlay.render();
        }
    }

    fn on_event(&mut self, event: UiEvent) {
        match &event {
            UiEvent::SettingsError { message } => {
                self.tray.warn("Glide 설정 파일 오류", message);
                return;
            }
            UiEvent::StateChanged => {
                self.tray.set_tip(&tip());
                return;
            }
            _ => {}
        }
        let Some(overlay) = &mut self.overlay else {
            return;
        };
        let settings = shared::settings();
        let rules = &settings.rules;
        // 결과 안내를 접고 오버레이를 숨긴다(필드를 따로 빌려 overlay와 함께 쓴다).
        let hide = |overlay: &mut Overlay, showing_result: &mut bool, window: HWND| {
            // SAFETY: 이 창의 타이머를 끈다.
            unsafe { KillTimer(window, HIDE_TIMER) };
            *showing_result = false;
            overlay.hide();
        };
        match event {
            UiEvent::Begin { x, y } => {
                // SAFETY: 이 창의 타이머를 끈다.
                unsafe { KillTimer(self.window, HIDE_TIMER) };
                self.showing_result = false;
                overlay.begin(x, y);
                // 방향이 정해지기 전에도 확장처럼 "계속 그리세요"를 보여 준다.
                overlay.set_hud(
                    String::new(),
                    gesture_hud(Sequence::default(), false, rules),
                );
            }
            UiEvent::Point { x, y } => overlay.add_point(x, y),
            UiEvent::Changed { sequence, invalid } => {
                overlay.set_hud(sequence.arrows(), gesture_hud(sequence, invalid, rules));
            }
            UiEvent::Finished { sequence, invalid } => {
                let registered =
                    !invalid && rules.iter().any(|rule| rule.gesture == sequence.as_str());
                if registered && overlay.is_visible() {
                    // 실행될 동작 안내만 남기고 궤적은 지운다.
                    overlay.clear_trail();
                    overlay.set_hud(sequence.arrows(), gesture_hud(sequence, false, rules));
                    self.showing_result = true;
                    // SAFETY: 이 창에 숨김 타이머를 건다(같은 ID면 교체된다).
                    unsafe { SetTimer(self.window, HIDE_TIMER, RESULT_MS, None) };
                } else {
                    // 등록되지 않은 제스처는 실행 결과가 없으므로 바로 닫는다(확장과 같음).
                    hide(overlay, &mut self.showing_result, self.window);
                }
            }
            UiEvent::Cancelled => hide(overlay, &mut self.showing_result, self.window),
            UiEvent::Notice { sequence, message } => {
                if self.showing_result {
                    overlay.set_hud(
                        sequence.arrows(),
                        Hud {
                            title: message.into(),
                            detail: String::new(),
                            tone: Tone::Error,
                        },
                    );
                    // SAFETY: 숨김 타이머를 실패 안내 시간으로 교체한다.
                    unsafe { SetTimer(self.window, HIDE_TIMER, NOTICE_MS, None) };
                }
            }
            UiEvent::SettingsError { .. } | UiEvent::StateChanged => {}
        }
    }

    fn hide_overlay(&mut self) {
        // SAFETY: 이 창의 타이머를 끈다.
        unsafe { KillTimer(self.window, HIDE_TIMER) };
        self.showing_result = false;
        if let Some(overlay) = &mut self.overlay {
            overlay.hide();
        }
    }
}

/// 훅 스레드에 우클릭 추적 초기화를 요청하고 오버레이를 숨긴다(UI 스레드에서 호출).
fn reset_tracking(reason: ResetReason) {
    with_ui(Ui::hide_overlay);
    let thread = HOOK_THREAD.load(Ordering::SeqCst);
    if thread != 0 {
        // SAFETY: 훅 스레드 ID로 스레드 메시지를 넣는다.
        unsafe { PostThreadMessageW(thread, WM_HOOK_RESET, reason as usize, 0) };
    }
}

/// 트레이 아이콘 도움말
fn tip() -> String {
    format!("Glide 제스처 · {}", state_text())
}

fn state_text() -> &'static str {
    if !RUNTIME_ENABLED.load(Ordering::Relaxed) {
        "꺼짐"
    } else if !SETTINGS_ENABLED.load(Ordering::Relaxed) {
        "설정에서 꺼짐"
    } else if FULLSCREEN_PAUSED.load(Ordering::Relaxed) {
        "전체 화면이라 일시 중지"
    } else {
        "사용 중"
    }
}

/// 트레이 메뉴. 모달 루프 동안 UI 상태를 빌리지 않도록 필요한 값만 먼저 읽는다.
fn show_menu(window: HWND) {
    let snapshot = hook::HOOK_LATENCY.snapshot();
    let p99 = match snapshot.percentile_upper_bound(99.0) {
        _ if snapshot.count() == 0 => "기록 없음".into(),
        Some(bound) => format!("<{bound}µs"),
        None => "10ms 이상".into(),
    };
    let status = format!(
        "상태: {} · 훅 p99 {p99} · 재설치 {}회",
        state_text(),
        hook::REINSTALLS.load(Ordering::Relaxed)
    );
    let admin_line = if UI_ACCESS.load(Ordering::Relaxed) {
        "관리자 권한 창: 지원"
    } else {
        "관리자 권한 창: 지원 안 함(설치판에서 지원)"
    };
    let enabled = RUNTIME_ENABLED.load(Ordering::Relaxed);
    // SAFETY: 메뉴를 만들어 띄우고 바로 해제한다. 메뉴 전에 창을 전면으로 두어야 바깥을 누르면 닫힌다.
    let command = unsafe {
        let menu = CreatePopupMenu();
        if menu.is_null() {
            return;
        }
        let item = |flags: u32, id: usize, text: &str| {
            AppendMenuW(menu, flags, id, wide(text).as_ptr());
        };
        item(
            MF_STRING | if enabled { MF_CHECKED } else { 0 },
            MENU_TOGGLE,
            "제스처 사용",
        );
        item(MF_STRING, MENU_OPEN_SETTINGS, "설정 열기");
        item(MF_STRING, MENU_OPEN_FOLDER, "설정 폴더 열기");
        AppendMenuW(menu, MF_SEPARATOR, 0, null());
        item(MF_STRING | MF_GRAYED, 0, &status);
        item(MF_STRING | MF_GRAYED, 0, admin_line);
        AppendMenuW(menu, MF_SEPARATOR, 0, null());
        item(MF_STRING, MENU_EXIT, "종료");
        let mut cursor = POINT { x: 0, y: 0 };
        GetCursorPos(&mut cursor);
        SetForegroundWindow(window);
        let command = TrackPopupMenu(
            menu,
            TPM_RETURNCMD | TPM_NONOTIFY | TPM_RIGHTBUTTON,
            cursor.x,
            cursor.y,
            0,
            window,
            null(),
        );
        PostMessageW(window, WM_NULL, 0, 0);
        DestroyMenu(menu);
        command as usize
    };
    match command {
        MENU_TOGGLE => {
            RUNTIME_ENABLED.store(!enabled, Ordering::SeqCst);
            log::info(|| format!("[트레이] 제스처 {}", if enabled { "끔" } else { "켬" }));
            with_ui(|ui| ui.tray.set_tip(&tip()));
        }
        MENU_OPEN_SETTINGS => open_settings(),
        MENU_OPEN_FOLDER => {
            if let Some(folder) = with_ui(|ui| ui.settings_folder.clone())
                && !launch::open(&folder.display().to_string(), None)
            {
                log::warn(|| "[트레이] 설정 폴더를 열지 못했습니다".into());
            }
        }
        MENU_EXIT => {
            // SAFETY: 이 스레드의 메시지 루프를 끝낸다.
            unsafe { PostQuitMessage(0) };
        }
        _ => {}
    }
}

/// 설정 창(같은 폴더의 glide-settings.exe)을 연다. 없으면(개발 빌드 등) settings.json을 메모장으로 연다.
fn open_settings() {
    let settings_app = std::env::current_exe()
        .ok()
        .and_then(|me| me.parent().map(|folder| folder.join(ipc::SETTINGS_EXE)))
        .filter(|path| path.is_file());
    let opened = match settings_app {
        Some(app) => launch::open(&app.display().to_string(), None),
        None => with_ui(|ui| ui.settings_file.clone()).is_some_and(|path| {
            // 연결 프로그램이 없는 PC도 있으므로 메모장으로 연다.
            launch::open("notepad.exe", Some(&format!("\"{}\"", path.display())))
        }),
    };
    if !opened {
        log::warn(|| "[트레이] 설정을 열지 못했습니다".into());
    }
}
