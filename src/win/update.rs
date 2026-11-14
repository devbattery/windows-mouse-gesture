//! 새 버전 확인(설정 `windows.checkUpdates`, 기본 켜짐).
//!
//! - 시작 2분 뒤 한 번, 그 뒤 24시간마다 GitHub Releases API에 최신 버전만 묻는다. 실패하면 6시간 뒤 다시 묻는다.
//! - 보내는 것은 HTTPS GET 요청과 User-Agent(앱 이름·버전)뿐이고, 받은 결과는 메모리에만 둔다.
//! - 설정이 꺼져 있으면 아무 통신도 하지 않는다.
//! - 새 버전이 있으면 트레이 알림을 버전마다 한 번 띄우고, 트레이 메뉴·설정 창에 표시한다.
//!   받기·설치는 사용자가 Release 페이지에서 한다(서명 전이라 앱이 직접 설치하지 않는다).
//! - 통신은 Windows 내장 WinHTTP(시스템 프록시·TLS 설정 사용)로 한다. 훅과 무관한 별도 스레드다.

use std::sync::Mutex;
use std::sync::mpsc::SyncSender;
use std::thread;
use std::time::{Duration, SystemTime};

use glide_core::update::{self, Version};
use windows_sys::Win32::Networking::WinHttp::{
    INTERNET_DEFAULT_HTTPS_PORT, WINHTTP_ACCESS_TYPE_AUTOMATIC_PROXY, WINHTTP_ADDREQ_FLAG_ADD,
    WINHTTP_FLAG_SECURE, WINHTTP_QUERY_FLAG_NUMBER, WINHTTP_QUERY_STATUS_CODE,
    WinHttpAddRequestHeaders, WinHttpCloseHandle, WinHttpConnect, WinHttpOpen, WinHttpOpenRequest,
    WinHttpQueryHeaders, WinHttpReadData, WinHttpReceiveResponse, WinHttpSendRequest,
    WinHttpSetTimeouts,
};

use super::ui::{self, UiEvent};
use super::{log, shared};

const FIRST_CHECK: Duration = Duration::from_secs(2 * 60);
const CHECK_INTERVAL: Duration = Duration::from_secs(24 * 60 * 60);
const RETRY_INTERVAL: Duration = Duration::from_secs(6 * 60 * 60);
/// 설정이 바뀌었는지(꺼짐 → 켜짐) 다시 보는 간격
const TICK: Duration = Duration::from_secs(10 * 60);
/// 이름 풀이·연결·보내기·받기 각각의 제한 시간(ms)
const TIMEOUT_MS: i32 = 15_000;

/// 확인한 최신 버전(지금보다 새 버전일 때만). 트레이 메뉴와 설정 창 상태가 읽는다.
static AVAILABLE: Mutex<Option<Version>> = Mutex::new(None);

/// 지금보다 새 버전이 확인됐으면 그 버전.
pub fn available() -> Option<Version> {
    AVAILABLE.lock().map_or(None, |guard| *guard)
}

fn current() -> Version {
    // Cargo.toml 버전은 빌드 때 정해지고 형식이 고정돼 있다.
    Version::parse(env!("CARGO_PKG_VERSION")).unwrap_or(Version {
        major: 0,
        minor: 0,
        patch: 0,
    })
}

/// 확인 스레드를 시작한다.
pub fn start(ui_events: SyncSender<UiEvent>) {
    let spawned = thread::Builder::new()
        .name("glide-update".into())
        .spawn(move || run(ui_events));
    if spawned.is_err() {
        log::warn(|| "[새 버전] 확인 스레드를 만들지 못했습니다".into());
    }
}

/// 다음 확인까지 남은 시간. 벽시계(SystemTime)로 재서 절전·최대 절전 시간도 간격에 들어가게 한다
/// (단조 시계는 절전 중에 멈출 수 있어, 밤마다 절전하는 PC에서는 확인이 며칠씩 밀린다).
/// 시계를 뒤로 돌려 남은 시간이 간격보다 길어지면 간격으로 줄인다.
fn remaining(next_check: &mut SystemTime) -> Duration {
    let now = SystemTime::now();
    match next_check.duration_since(now) {
        Ok(left) if left > CHECK_INTERVAL => {
            *next_check = now + CHECK_INTERVAL;
            CHECK_INTERVAL
        }
        Ok(left) => left,
        Err(_) => Duration::ZERO,
    }
}

fn run(ui_events: SyncSender<UiEvent>) {
    let mut next_check = SystemTime::now() + FIRST_CHECK;
    // 이미 알림을 띄운 버전(같은 버전으로 다시 알리지 않는다)
    let mut notified: Option<Version> = None;
    loop {
        // 첫 확인까지는 짧게, 그 뒤로는 TICK마다 깨어나 설정·시각을 다시 본다.
        let left = remaining(&mut next_check);
        thread::sleep(if left.is_zero() { TICK } else { left.min(TICK) });
        if !shared::settings().check_updates || !remaining(&mut next_check).is_zero() {
            continue;
        }
        match check() {
            Ok(latest) => {
                next_check = SystemTime::now() + CHECK_INTERVAL;
                let newer = update::is_newer(current(), latest).then_some(latest);
                if let Ok(mut guard) = AVAILABLE.lock() {
                    *guard = newer;
                }
                log::info(|| format!("[새 버전] 최신 {latest} · 이 PC {}", current()));
                if let Some(version) = newer
                    && notified != Some(version)
                    && ui_events
                        .try_send(UiEvent::UpdateAvailable { version })
                        .is_ok()
                {
                    notified = Some(version);
                    ui::wake();
                }
            }
            Err(message) => {
                next_check = SystemTime::now() + RETRY_INTERVAL;
                log::warn(|| format!("[새 버전] 확인하지 못했습니다: {message}"));
            }
        }
    }
}

/// 최신 버전을 한 번 묻는다(`--check-update` 진단에서도 쓴다).
pub fn check() -> Result<Version, String> {
    let body = fetch(update::API_HOST, update::API_PATH)?;
    update::parse_latest(&body)
}

/// 진단용: 한 번 확인해 결과 문장을 돌려준다.
pub fn check_report() -> (bool, String) {
    match check() {
        Ok(latest) if update::is_newer(current(), latest) => (
            true,
            format!(
                "새 버전 {latest}이(가) 있습니다(이 PC {}). {}",
                current(),
                update::release_url(latest)
            ),
        ),
        Ok(latest) => (
            true,
            format!(
                "최신 버전입니다(이 PC {}, GitHub 최신 {latest}).",
                current()
            ),
        ),
        Err(message) => (false, format!("새 버전을 확인하지 못했습니다: {message}")),
    }
}

/// WinHTTP 핸들. 범위를 벗어나면 닫는다.
struct Handle(*mut core::ffi::c_void);

impl Handle {
    fn new(raw: *mut core::ffi::c_void, what: &str) -> Result<Self, String> {
        if raw.is_null() {
            Err(format!("{what} 실패({})", last_error()))
        } else {
            Ok(Self(raw))
        }
    }
}

impl Drop for Handle {
    fn drop(&mut self) {
        // SAFETY: WinHttpOpen·Connect·OpenRequest가 돌려준 핸들을 한 번만 닫는다.
        unsafe { WinHttpCloseHandle(self.0) };
    }
}

fn last_error() -> u32 {
    // SAFETY: 이 스레드의 마지막 오류 코드를 읽기만 한다.
    unsafe { windows_sys::Win32::Foundation::GetLastError() }
}

fn wide(text: &str) -> Vec<u16> {
    text.encode_utf16().chain(Some(0)).collect()
}

/// HTTPS GET으로 본문을 받는다. 상태 200만 성공이고, 본문은 `MAX_RESPONSE_BYTES`까지만 읽는다.
fn fetch(host: &str, path: &str) -> Result<Vec<u8>, String> {
    let agent = wide(&format!("Glide-for-Windows/{}", env!("CARGO_PKG_VERSION")));
    let host = wide(host);
    let path = wide(path);
    let verb = wide("GET");
    let headers = wide("Accept: application/vnd.github+json\r\n");
    // SAFETY: 널 종료 UTF-16 문자열과 이 함수 안에서만 쓰는 핸들로 WinHTTP를 호출한다.
    // 핸들은 Handle이 닫고, 요청 → 연결 → 세션 순서(선언의 역순)로 해제된다.
    unsafe {
        let session = Handle::new(
            WinHttpOpen(
                agent.as_ptr(),
                WINHTTP_ACCESS_TYPE_AUTOMATIC_PROXY,
                std::ptr::null(),
                std::ptr::null(),
                0,
            ),
            "WinHttpOpen",
        )?;
        WinHttpSetTimeouts(session.0, TIMEOUT_MS, TIMEOUT_MS, TIMEOUT_MS, TIMEOUT_MS);
        let connection = Handle::new(
            WinHttpConnect(session.0, host.as_ptr(), INTERNET_DEFAULT_HTTPS_PORT, 0),
            "연결 준비",
        )?;
        let request = Handle::new(
            WinHttpOpenRequest(
                connection.0,
                verb.as_ptr(),
                path.as_ptr(),
                std::ptr::null(),
                std::ptr::null(),
                std::ptr::null(),
                WINHTTP_FLAG_SECURE,
            ),
            "요청 준비",
        )?;
        WinHttpAddRequestHeaders(
            request.0,
            headers.as_ptr(),
            u32::MAX, // 널 종료 문자열 길이를 자동으로 계산
            WINHTTP_ADDREQ_FLAG_ADD,
        );
        if WinHttpSendRequest(request.0, std::ptr::null(), 0, std::ptr::null(), 0, 0, 0) == 0 {
            return Err(format!("요청을 보내지 못했습니다({})", last_error()));
        }
        if WinHttpReceiveResponse(request.0, std::ptr::null_mut()) == 0 {
            return Err(format!("응답을 받지 못했습니다({})", last_error()));
        }
        let mut status: u32 = 0;
        let mut size = size_of::<u32>() as u32;
        if WinHttpQueryHeaders(
            request.0,
            WINHTTP_QUERY_STATUS_CODE | WINHTTP_QUERY_FLAG_NUMBER,
            std::ptr::null(),
            (&mut status as *mut u32).cast(),
            &mut size,
            std::ptr::null_mut(),
        ) == 0
        {
            return Err(format!("응답 상태를 읽지 못했습니다({})", last_error()));
        }
        if status != 200 {
            return Err(format!("GitHub 응답 상태 {status}"));
        }
        let mut body = Vec::new();
        let mut buffer = [0u8; 8192];
        loop {
            let mut read: u32 = 0;
            if WinHttpReadData(
                request.0,
                buffer.as_mut_ptr().cast(),
                buffer.len() as u32,
                &mut read,
            ) == 0
            {
                return Err(format!("응답을 읽지 못했습니다({})", last_error()));
            }
            if read == 0 {
                break;
            }
            body.extend_from_slice(&buffer[..read as usize]);
            if body.len() > update::MAX_RESPONSE_BYTES {
                return Err("응답이 너무 큽니다.".into());
            }
        }
        Ok(body)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 실제 GitHub에 묻는 시험. 네트워크가 필요해 기본으로는 건너뛴다.
    /// `cargo test -p glide-core --bin glide-core -- --ignored live` 로 실행한다.
    #[test]
    #[ignore = "네트워크 필요"]
    fn live_github_check() {
        let latest = check().expect("GitHub 최신 버전 확인");
        println!("GitHub 최신 {latest}, 이 빌드 {}", current());
        println!("{}", check_report().1);
    }

    /// 연결할 수 없는 주소는 오류로 끝난다(.invalid는 실제로 풀리지 않는 예약 도메인). 네트워크 설정에 따라
    /// 시간이 걸릴 수 있어 기본으로는 건너뛴다.
    #[test]
    #[ignore = "네트워크 필요"]
    fn live_unreachable_host() {
        assert!(fetch("glide-update-check.invalid", "/").is_err());
    }
}
