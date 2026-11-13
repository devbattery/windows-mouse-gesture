//! 고해상도 시계(QueryPerformanceCounter). 훅 처리 시간을 마이크로초 단위로 잰다.

use std::sync::OnceLock;
use windows_sys::Win32::System::Performance::{QueryPerformanceCounter, QueryPerformanceFrequency};

static FREQUENCY: OnceLock<i64> = OnceLock::new();

/// 훅을 설치하기 전에 한 번 불러 주파수를 캐시한다(훅 안에서 OnceLock 초기화를 피함).
pub fn init() {
    frequency();
}

fn frequency() -> i64 {
    *FREQUENCY.get_or_init(|| {
        let mut value = 0;
        // SAFETY: 유효한 i64 포인터를 넘긴다. Windows XP 이후에는 항상 성공한다.
        unsafe { QueryPerformanceFrequency(&mut value) };
        value.max(1)
    })
}

pub fn now() -> i64 {
    let mut value = 0;
    // SAFETY: 유효한 i64 포인터를 넘긴다.
    unsafe { QueryPerformanceCounter(&mut value) };
    value
}

pub fn micros_since(start: i64) -> u64 {
    let elapsed = (now() - start).max(0) as u128;
    (elapsed * 1_000_000 / frequency() as u128) as u64
}
