//! 훅 콜백 처리 시간 히스토그램.
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

#[cfg(test)]
mod tests {
    use super::*;

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
