//! 훅 콜백 처리 시간·누름 시간 히스토그램.
//!
//! M1 성능 게이트(훅 p99 1ms 미만)를 재기 위한 것이다. 훅 스레드가 기록하고 다른
//! 스레드가 읽으므로 원자적 카운터만 쓰고, 잠금·할당이 없다.

use std::sync::atomic::{AtomicU64, Ordering};

/// 구간 상한(마이크로초). 마지막 구간은 상한이 없다.
pub const BOUNDS_US: [u64; 9] = [25, 50, 100, 250, 500, 1_000, 2_000, 5_000, 10_000];

pub struct LatencyHistogram {
    buckets: [AtomicU64; BOUNDS_US.len() + 1],
    max_us: AtomicU64,
}

/// 특정 시점에 읽은 히스토그램 값.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Snapshot {
    pub buckets: [u64; BOUNDS_US.len() + 1],
    pub max_us: u64,
}

impl LatencyHistogram {
    pub const fn new() -> Self {
        // const 문맥에서 배열을 만들기 위해 상수를 반복한다.
        #[allow(clippy::declare_interior_mutable_const)]
        const ZERO: AtomicU64 = AtomicU64::new(0);
        Self {
            buckets: [ZERO; BOUNDS_US.len() + 1],
            max_us: AtomicU64::new(0),
        }
    }

    pub fn record(&self, micros: u64) {
        let index = BOUNDS_US
            .iter()
            .position(|bound| micros < *bound)
            .unwrap_or(BOUNDS_US.len());
        self.buckets[index].fetch_add(1, Ordering::Relaxed);
        self.max_us.fetch_max(micros, Ordering::Relaxed);
    }

    pub fn snapshot(&self) -> Snapshot {
        let mut buckets = [0; BOUNDS_US.len() + 1];
        for (slot, bucket) in buckets.iter_mut().zip(&self.buckets) {
            *slot = bucket.load(Ordering::Relaxed);
        }
        Snapshot {
            buckets,
            max_us: self.max_us.load(Ordering::Relaxed),
        }
    }
}

impl Default for LatencyHistogram {
    fn default() -> Self {
        Self::new()
    }
}

impl Snapshot {
    pub fn count(&self) -> u64 {
        self.buckets.iter().sum()
    }

    /// 백분위 `percent`(0~100)가 들어 있는 구간의 상한(µs). 마지막 구간이면 None(상한 없음).
    /// 구간 단위 추정이라 실제 값은 이 상한보다 작거나 같다.
    pub fn percentile_upper_bound(&self, percent: f64) -> Option<u64> {
        let total = self.count();
        if total == 0 {
            return Some(0);
        }
        let target = (total as f64 * percent / 100.0).ceil() as u64;
        let mut cumulative = 0;
        for (index, count) in self.buckets.iter().enumerate() {
            cumulative += count;
            if cumulative >= target {
                return BOUNDS_US.get(index).copied();
            }
        }
        None
    }

    /// 사람이 읽을 한 줄 요약.
    pub fn summary(&self) -> String {
        if self.count() == 0 {
            return "훅 호출 0회(아직 마우스 입력 없음)".into();
        }
        let bound = |percent| match self.percentile_upper_bound(percent) {
            Some(us) => format!("<{us}µs"),
            None => format!(">={}µs", BOUNDS_US[BOUNDS_US.len() - 1]),
        };
        format!(
            "훅 호출 {}회 · p50 {} · p99 {} · 최대 {}µs · 1ms 이상 {}회",
            self.count(),
            bound(50.0),
            bound(99.0),
            self.max_us,
            self.over(1_000)
        )
    }

    /// `micros` 이상 걸린 호출 수.
    pub fn over(&self, micros: u64) -> u64 {
        let start = BOUNDS_US
            .iter()
            .position(|bound| *bound == micros)
            .map_or(BOUNDS_US.len(), |index| index + 1);
        self.buckets[start..].iter().sum()
    }
}

/// 누름 시간 구간 폭(ms)과 구간 수. 0~2000ms를 25ms 단위로 나누고, 마지막 구간은 상한이 없다.
pub const PRESS_STEP_MS: u64 = 25;
const PRESS_BUCKETS: usize = 80;
/// 추천값을 내기 위한 최소 표본 수
pub const MIN_SAMPLES: u64 = 20;

/// 누름 시간 히스토그램(ms). 길게 누르기 기준(`holdToPassMs`)을 정하는 데 쓴다.
/// 훅 스레드가 기록하고 파이프 스레드가 읽는다. 메모리에만 있고 재시작하면 비워진다.
pub struct PressHistogram {
    buckets: [AtomicU64; PRESS_BUCKETS + 1],
}

impl PressHistogram {
    pub const fn new() -> Self {
        #[allow(clippy::declare_interior_mutable_const)]
        const ZERO: AtomicU64 = AtomicU64::new(0);
        Self {
            buckets: [ZERO; PRESS_BUCKETS + 1],
        }
    }

    pub fn record(&self, millis: u64) {
        let index = ((millis / PRESS_STEP_MS) as usize).min(PRESS_BUCKETS);
        self.buckets[index].fetch_add(1, Ordering::Relaxed);
    }

    pub fn snapshot(&self) -> PressSnapshot {
        PressSnapshot {
            buckets: self
                .buckets
                .iter()
                .map(|bucket| bucket.load(Ordering::Relaxed))
                .collect(),
        }
    }
}

impl Default for PressHistogram {
    fn default() -> Self {
        Self::new()
    }
}

/// 특정 시점에 읽은 누름 시간 히스토그램.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct PressSnapshot {
    buckets: Vec<u64>,
}

impl PressSnapshot {
    pub fn count(&self) -> u64 {
        self.buckets.iter().sum()
    }

    /// 백분위 `percent`가 들어 있는 구간의 상한(ms). 표본이 없으면 None,
    /// 마지막 구간(2000ms 이상)이면 `u64::MAX`.
    pub fn percentile_upper_bound(&self, percent: f64) -> Option<u64> {
        let total = self.count();
        if total == 0 {
            return None;
        }
        let target = ((total as f64 * percent / 100.0).ceil() as u64).max(1);
        let mut cumulative = 0;
        for (index, count) in self.buckets.iter().enumerate() {
            cumulative += count;
            if cumulative >= target {
                return Some(if index < PRESS_BUCKETS {
                    (index as u64 + 1) * PRESS_STEP_MS
                } else {
                    u64::MAX
                });
            }
        }
        Some(u64::MAX)
    }
}

/// 길게 누르기 기준 추천값(ms).
///
/// 오른쪽을 누르고 제스처 임계값만큼 움직이기까지 걸린 시간의 p95에 여유 150ms를 더하고
/// 50ms 단위로 올린 값이다(250~1500ms). 기준이 이보다 짧으면 천천히 시작한 제스처가
/// 길게 누르기로 바뀌고, 길면 오른쪽 드래그를 쓰는 앱에서 기다리는 시간만 늘어난다.
/// 단순 클릭은 기준보다 오래 눌러도 원래 우클릭으로 전달되므로 추천값에 쓰지 않는다.
/// 표본이 [`MIN_SAMPLES`]개 미만이면 None.
pub fn recommend_hold_ms(gesture_start: &PressSnapshot) -> Option<u32> {
    if gesture_start.count() < MIN_SAMPLES {
        return None;
    }
    let p95 = gesture_start.percentile_upper_bound(95.0)?;
    let raw = p95.saturating_add(150);
    let rounded = raw.div_ceil(50).saturating_mul(50);
    Some(rounded.clamp(250, 1_500) as u32)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn press_histogram_reports_bucket_upper_bounds() {
        let histogram = PressHistogram::new();
        assert_eq!(histogram.snapshot().percentile_upper_bound(95.0), None);
        for _ in 0..19 {
            histogram.record(60);
        }
        histogram.record(240);
        let snapshot = histogram.snapshot();
        assert_eq!(snapshot.count(), 20);
        // 60ms는 50~75ms 구간, 240ms는 225~250ms 구간
        assert_eq!(snapshot.percentile_upper_bound(50.0), Some(75));
        assert_eq!(snapshot.percentile_upper_bound(100.0), Some(250));
        histogram.record(9_000);
        assert_eq!(
            histogram.snapshot().percentile_upper_bound(100.0),
            Some(u64::MAX)
        );
    }

    #[test]
    fn hold_recommendation_needs_enough_samples_and_stays_in_range() {
        let histogram = PressHistogram::new();
        for _ in 0..19 {
            histogram.record(100);
        }
        assert_eq!(recommend_hold_ms(&histogram.snapshot()), None);
        histogram.record(100);
        // p95 상한 125ms + 150 = 275 → 300ms
        assert_eq!(recommend_hold_ms(&histogram.snapshot()), Some(300));

        let fast = PressHistogram::new();
        for _ in 0..30 {
            fast.record(10);
        }
        assert_eq!(recommend_hold_ms(&fast.snapshot()), Some(250));

        let slow = PressHistogram::new();
        for _ in 0..30 {
            slow.record(5_000);
        }
        assert_eq!(recommend_hold_ms(&slow.snapshot()), Some(1_500));
    }

    #[test]
    fn percentiles_follow_buckets() {
        let histogram = LatencyHistogram::new();
        for _ in 0..98 {
            histogram.record(10);
        }
        histogram.record(700);
        histogram.record(3_000);
        let snapshot = histogram.snapshot();
        assert_eq!(snapshot.count(), 100);
        assert_eq!(snapshot.percentile_upper_bound(50.0), Some(25));
        assert_eq!(snapshot.percentile_upper_bound(99.0), Some(1_000));
        assert_eq!(snapshot.max_us, 3_000);
        assert_eq!(snapshot.over(1_000), 1);
    }

    #[test]
    fn slowest_bucket_has_no_upper_bound() {
        let histogram = LatencyHistogram::new();
        histogram.record(50_000);
        assert_eq!(histogram.snapshot().percentile_upper_bound(99.0), None);
    }

    #[test]
    fn empty_histogram_reports_zero() {
        assert_eq!(Snapshot::default().percentile_upper_bound(99.0), Some(0));
    }
}
