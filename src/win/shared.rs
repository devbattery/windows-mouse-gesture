//! 스레드 사이에 공유하는 상태.
//!
//! 훅 스레드는 잠금 없이 읽어야 하므로 원자 값만 쓴다. 규칙 목록처럼 큰 값은 워커·UI 스레드만
//! 읽으며, `RwLock<Arc<_>>` 스냅샷을 짧게 복제해 쓴다.

use std::sync::atomic::{AtomicBool, AtomicIsize, AtomicU32, Ordering};
use std::sync::{Arc, LazyLock, Mutex, RwLock};

use glide_core::history::{Entry, History};
use glide_core::settings::Settings;
use windows_sys::Win32::UI::WindowsAndMessaging::WM_APP;

/// 트레이 메뉴의 "제스처 사용"(실행 중에만 유지)
pub static RUNTIME_ENABLED: AtomicBool = AtomicBool::new(true);
/// 설정 파일의 enabled
pub static SETTINGS_ENABLED: AtomicBool = AtomicBool::new(true);
/// 전체 화면 게임·프레젠테이션으로 자동 일시 중지 중
pub static FULLSCREEN_PAUSED: AtomicBool = AtomicBool::new(false);
pub static THRESHOLD: AtomicU32 = AtomicU32::new(glide_core::gesture::DEFAULT_THRESHOLD);
pub static HOLD_MS: AtomicU32 = AtomicU32::new(glide_core::settings::DEFAULT_HOLD_MS);
/// 이 프로세스가 uiAccess 토큰으로 실행됐는지(관리자 권한 창에서도 동작). 시작할 때 한 번 정한다.
pub static UI_ACCESS: AtomicBool = AtomicBool::new(false);
/// 다른 프로그램이 주입한 입력도 제스처로 해석(설정 `allowInjectedInput`)
pub static ALLOW_INJECTED: AtomicBool = AtomicBool::new(false);

/// 훅 스레드 ID(스레드 메시지 대상)
pub static HOOK_THREAD: AtomicU32 = AtomicU32::new(0);
/// UI 스레드의 제어 창(트레이 창). 메뉴 같은 모달 루프 중에도 메시지가 사라지지 않도록
/// 스레드 메시지 대신 이 창으로 보낸다.
pub static UI_WINDOW: AtomicIsize = AtomicIsize::new(0);

/// 훅 스레드로: 예외 판정 결과(wParam = PID, lParam = 1이면 예외)
pub const WM_HOOK_EXCLUSION: u32 = WM_APP + 1;
/// 훅 스레드로: 예외 판정 캐시 비우기(설정이 바뀜)
pub const WM_HOOK_CLEAR_EXCLUSIONS: u32 = WM_APP + 2;
/// 훅 스레드로: 진행 중인 우클릭 추적을 버리고 훅을 다시 설치(wParam = [`ResetReason`])
pub const WM_HOOK_RESET: u32 = WM_APP + 3;

/// 초기화 이유(로그용).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(usize)]
pub enum ResetReason {
    /// 화면 잠금·해제, 원격 연결·해제, 빠른 사용자 전환
    Session = 1,
    /// 절전에서 복귀
    Resume = 2,
    /// 모니터 연결·해상도 변경
    Display = 3,
    /// 제스처 중 입력 데스크톱이 바뀜(UAC 확인 창, Ctrl+Alt+Del)
    SecureDesktop = 4,
}

impl ResetReason {
    pub fn from_wparam(value: usize) -> Self {
        match value {
            1 => Self::Session,
            2 => Self::Resume,
            3 => Self::Display,
            _ => Self::SecureDesktop,
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::Session => "세션 변경(잠금·원격 연결 등)",
            Self::Resume => "절전 복귀",
            Self::Display => "디스플레이 변경",
            Self::SecureDesktop => "보안 데스크톱 전환",
        }
    }
}
/// UI 창으로: 채널에 새 사건이 있음
pub const WM_UI_WAKE: u32 = WM_APP + 10;
/// UI 창으로: 종료 요청(Ctrl+C 등)
pub const WM_UI_EXIT: u32 = WM_APP + 11;
/// 트레이 아이콘 알림
pub const WM_UI_TRAY: u32 = WM_APP + 12;

static SETTINGS: LazyLock<RwLock<Arc<Settings>>> =
    LazyLock::new(|| RwLock::new(Arc::new(Settings::default())));

/// 설정 파일의 마지막 오류(올바르지 않은 값 등). 설정 창이 보여 준다. 정상으로 읽으면 비운다.
static SETTINGS_ERROR: Mutex<Option<String>> = Mutex::new(None);

/// 최근 우클릭 기록(메모리에만). 워커가 쓰고 파이프 스레드가 읽는다. 훅 스레드는 건드리지 않는다.
static HISTORY: LazyLock<Mutex<History>> = LazyLock::new(|| Mutex::new(History::default()));

pub fn record(entry: Entry) {
    match HISTORY.lock() {
        Ok(mut guard) => guard.push(entry),
        Err(poisoned) => poisoned.into_inner().push(entry),
    }
}

/// 최근 기록을 설정 창용 JSON으로 읽는다.
pub fn history_value() -> serde_json::Value {
    match HISTORY.lock() {
        Ok(guard) => guard.to_value(),
        Err(poisoned) => poisoned.into_inner().to_value(),
    }
}

pub fn settings_error() -> Option<String> {
    SETTINGS_ERROR.lock().map_or_else(
        |poisoned| poisoned.into_inner().clone(),
        |guard| guard.clone(),
    )
}

pub fn set_settings_error(message: Option<String>) {
    match SETTINGS_ERROR.lock() {
        Ok(mut guard) => *guard = message,
        Err(poisoned) => *poisoned.into_inner() = message,
    }
}

/// 현재 설정 스냅샷.
pub fn settings() -> Arc<Settings> {
    SETTINGS.read().map_or_else(
        |poisoned| poisoned.into_inner().clone(),
        |guard| guard.clone(),
    )
}

/// 검증된 설정을 모든 스레드에 반영한다.
pub fn publish(settings: Settings) {
    SETTINGS_ENABLED.store(settings.enabled, Ordering::SeqCst);
    THRESHOLD.store(settings.threshold, Ordering::SeqCst);
    HOLD_MS.store(settings.hold_ms, Ordering::SeqCst);
    ALLOW_INJECTED.store(settings.allow_injected_input, Ordering::SeqCst);
    if !settings.auto_pause_fullscreen {
        FULLSCREEN_PAUSED.store(false, Ordering::SeqCst);
    }
    let snapshot = Arc::new(settings);
    match SETTINGS.write() {
        Ok(mut guard) => *guard = snapshot,
        Err(poisoned) => *poisoned.into_inner() = snapshot,
    }
}

/// 지금 새 제스처를 시작해도 되는지(보조 키·예외 앱 판정 전 단계).
pub fn gestures_active() -> bool {
    RUNTIME_ENABLED.load(Ordering::Relaxed)
        && SETTINGS_ENABLED.load(Ordering::Relaxed)
        && !FULLSCREEN_PAUSED.load(Ordering::Relaxed)
}
