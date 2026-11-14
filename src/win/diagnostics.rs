//! 일상 사용 점검(`getDiagnostics`) 응답.
//!
//! 설정 창의 "일상 사용 점검" 칸이 쓴다. 최근 우클릭 기록, 누름 시간 통계와 길게 누르기
//! 추천값, 문제를 알릴 때 붙일 진단 정보를 담는다. 좌표·창 제목은 넣지 않는다.

use std::sync::atomic::Ordering;
use std::time::Instant;

use glide_core::settings::SUGGESTED_EXCLUSIONS;
use glide_core::stats::{self, PressSnapshot};
use serde_json::{Value, json};
use std::sync::LazyLock;
use windows_sys::Win32::Foundation::{LPARAM, RECT};
use windows_sys::Win32::Graphics::Gdi::{
    EnumDisplayMonitors, GetMonitorInfoW, HDC, HMONITOR, MONITORINFO,
};
use windows_sys::Win32::System::Registry::{
    HKEY_LOCAL_MACHINE, RRF_RT_REG_DWORD, RRF_RT_REG_SZ, RegGetValueW,
};
use windows_sys::Win32::UI::HiDpi::{GetDpiForMonitor, MDT_EFFECTIVE_DPI};
use windows_sys::Win32::UI::WindowsAndMessaging::MONITORINFOF_PRIMARY;
use windows_sys::core::BOOL;

use super::hook;
use super::shared::{self, ALLOW_INJECTED, FULLSCREEN_PAUSED, HOLD_MS, UI_ACCESS};

/// 코어 시작 시각(실행 시간 표시용). `main`에서 처음 읽어 고정한다.
pub static STARTED: LazyLock<Instant> = LazyLock::new(Instant::now);

/// 진단 응답 본문(`ok`는 호출 측이 붙인다).
pub fn collect() -> Value {
    let settings = shared::settings();
    let click = hook::CLICK_PRESS.snapshot();
    let gesture_start = hook::GESTURE_START.snapshot();
    let latency = hook::HOOK_LATENCY.snapshot();
    let suggested: Vec<Value> = SUGGESTED_EXCLUSIONS
        .iter()
        .map(|(name, reason)| json!({ "name": name, "reason": reason }))
        .collect();
    json!({
        "history": shared::history_value(),
        "press": {
            "clickCount": click.count(),
            "clickP50": bound(&click, 50.0),
            "clickP95": bound(&click, 95.0),
            "gestureStartCount": gesture_start.count(),
            "gestureStartP95": bound(&gesture_start, 95.0),
            "recommendedHoldMs": stats::recommend_hold_ms(&gesture_start),
            "currentHoldMs": HOLD_MS.load(Ordering::Relaxed),
            "minSamples": stats::MIN_SAMPLES,
            "bucketMs": stats::PRESS_STEP_MS,
        },
        "suggestedExclusions": suggested,
        "system": {
            "version": env!("CARGO_PKG_VERSION"),
            "os": os_version(),
            "uiAccess": UI_ACCESS.load(Ordering::Relaxed),
            "uptimeSeconds": STARTED.elapsed().as_secs(),
            "monitors": monitors(),
            "hookCalls": latency.count(),
            "hookP99Micros": if latency.count() == 0 { Some(0) } else { latency.percentile_upper_bound(99.0) },
            "hookOver1ms": latency.over(1_000),
            "hookOver10ms": latency.over(10_000),
            "droppedEvents": hook::DROPPED_EVENTS.load(Ordering::Relaxed),
            "reinstalls": hook::REINSTALLS.load(Ordering::Relaxed),
            "resets": hook::RESETS.load(Ordering::Relaxed),
            "fullscreenPaused": FULLSCREEN_PAUSED.load(Ordering::Relaxed),
            "allowInjectedInput": ALLOW_INJECTED.load(Ordering::Relaxed),
            "ruleCount": settings.rules.len(),
            "excludedCount": settings.excluded_processes.len(),
            "thresholdPx": settings.threshold,
            "holdMs": settings.hold_ms,
            "autoPauseFullscreen": settings.auto_pause_fullscreen,
            "checkUpdates": settings.check_updates,
        },
    })
}

/// 백분위 구간 상한(ms). 표본이 없으면 null, 2000ms 이상이면 -1(설정 창이 "2초 이상"으로 보여 줌).
fn bound(snapshot: &PressSnapshot, percent: f64) -> Value {
    match snapshot.percentile_upper_bound(percent) {
        None => Value::Null,
        Some(u64::MAX) => json!(-1),
        Some(ms) => json!(ms),
    }
}

/// "Windows 10.0.19045.6456" 형식. 레지스트리의 빌드 번호를 읽는다
/// (GetVersionEx는 매니페스트 없이 실행하면 실제 버전을 알려 주지 않는다).
fn os_version() -> String {
    const KEY: &str = r"SOFTWARE\Microsoft\Windows NT\CurrentVersion";
    let build = registry_string(KEY, "CurrentBuildNumber").unwrap_or_default();
    let revision = registry_dword(KEY, "UBR");
    let display = registry_string(KEY, "DisplayVersion").unwrap_or_default();
    let mut text = format!("Windows 10.0.{build}");
    if let Some(revision) = revision {
        text.push_str(&format!(".{revision}"));
    }
    if !display.is_empty() {
        text.push_str(&format!(" ({display})"));
    }
    text
}

fn wide(text: &str) -> Vec<u16> {
    text.encode_utf16().chain(std::iter::once(0)).collect()
}

fn registry_string(key: &str, name: &str) -> Option<String> {
    let (key, name) = (wide(key), wide(name));
    let mut buffer = [0u16; 128];
    let mut size = (buffer.len() * 2) as u32;
    // SAFETY: 널 종료 이름과 크기를 아는 버퍼를 넘긴다. 문자열 값만 받는다.
    let status = unsafe {
        RegGetValueW(
            HKEY_LOCAL_MACHINE,
            key.as_ptr(),
            name.as_ptr(),
            RRF_RT_REG_SZ,
            std::ptr::null_mut(),
            buffer.as_mut_ptr().cast(),
            &mut size,
        )
    };
    if status != 0 {
        return None;
    }
    let length = (size as usize / 2).min(buffer.len());
    let text = String::from_utf16_lossy(&buffer[..length]);
    Some(text.trim_end_matches('\0').to_string())
}

fn registry_dword(key: &str, name: &str) -> Option<u32> {
    let (key, name) = (wide(key), wide(name));
    let mut value: u32 = 0;
    let mut size = size_of::<u32>() as u32;
    // SAFETY: 널 종료 이름과 u32 출력 버퍼를 넘긴다.
    let status = unsafe {
        RegGetValueW(
            HKEY_LOCAL_MACHINE,
            key.as_ptr(),
            name.as_ptr(),
            RRF_RT_REG_DWORD,
            std::ptr::null_mut(),
            (&mut value as *mut u32).cast(),
            &mut size,
        )
    };
    (status == 0).then_some(value)
}

/// 모니터마다 해상도(물리 픽셀)·DPI·주 모니터 여부. 위치는 넣지 않는다.
fn monitors() -> Vec<Value> {
    unsafe extern "system" fn each(monitor: HMONITOR, _: HDC, _: *mut RECT, data: LPARAM) -> BOOL {
        // SAFETY: data는 아래에서 넘긴 Vec<Value>의 포인터이고, 이 열거 동안만 쓴다.
        let list = unsafe { &mut *(data as *mut Vec<Value>) };
        let mut info = MONITORINFO {
            cbSize: size_of::<MONITORINFO>() as u32,
            // SAFETY: 0으로 초기화해도 유효한 C 구조체다.
            ..unsafe { std::mem::zeroed() }
        };
        let (mut dpi_x, mut dpi_y) = (96, 96);
        // SAFETY: 열거 중 받은 모니터 핸들을 조회한다.
        unsafe {
            if GetMonitorInfoW(monitor, &mut info) == 0 {
                return 1;
            }
            if GetDpiForMonitor(monitor, MDT_EFFECTIVE_DPI, &mut dpi_x, &mut dpi_y) < 0 {
                dpi_x = 96;
            }
        }
        let rect = info.rcMonitor;
        list.push(json!({
            "width": rect.right - rect.left,
            "height": rect.bottom - rect.top,
            "scalePercent": dpi_x * 100 / 96,
            "primary": info.dwFlags & MONITORINFOF_PRIMARY != 0,
        }));
        1
    }
    let mut list: Vec<Value> = Vec::new();
    // SAFETY: 열거 콜백에 이 함수의 지역 Vec 포인터를 넘긴다. EnumDisplayMonitors는 동기적이다.
    unsafe {
        EnumDisplayMonitors(
            std::ptr::null_mut(),
            std::ptr::null(),
            Some(each),
            &mut list as *mut Vec<Value> as LPARAM,
        )
    };
    list
}

#[cfg(test)]
mod tests {
    use super::*;
    use glide_core::history::{CAPACITY, Entry, Kind};
    use glide_core::ipc;

    #[test]
    fn full_history_fits_in_one_pipe_message() {
        // 가장 긴 실행 파일 이름(260자)으로 기록을 가득 채워도 파이프 한 건(64KB)에 들어가야 한다.
        for at in 0..CAPACITY as u64 {
            shared::record(Entry {
                at,
                app: format!("{}.exe", "가".repeat(256)),
                kind: Kind::Failed,
                detail: Some("제스처를 시작한 창을 활성화하지 못했습니다".into()),
            });
        }
        let value = ipc::success(collect());
        let size = serde_json::to_vec(&value).unwrap().len();
        assert!(size < ipc::MAX_MESSAGE, "진단 응답이 {size}바이트입니다");
        assert_eq!(
            value["history"]["entries"].as_array().unwrap().len(),
            CAPACITY
        );
        assert!(
            value["system"]["os"]
                .as_str()
                .unwrap()
                .starts_with("Windows 10.0.")
        );
        assert!(!value["system"]["monitors"].as_array().unwrap().is_empty());
        assert_eq!(
            value["suggestedExclusions"].as_array().unwrap().len(),
            SUGGESTED_EXCLUSIONS.len()
        );
    }
}
