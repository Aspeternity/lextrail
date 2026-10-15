//! Summary statistics for timing samples.

use std::fmt;
use std::time::Duration;

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Summary {
    pub count: usize,
    pub min: Duration,
    pub median: Duration,
    pub p90: Duration,
    pub max: Duration,
}

impl Summary {
    /// `None` for an empty sample set.
    pub fn of(samples: &[Duration]) -> Option<Self> {
        if samples.is_empty() {
            return None;
        }
        let mut sorted = samples.to_vec();
        sorted.sort_unstable();
        Some(Self {
            count: sorted.len(),
            min: sorted[0],
            median: percentile(&sorted, 50),
            p90: percentile(&sorted, 90),
            max: sorted[sorted.len() - 1],
        })
    }
}

/// Nearest-rank percentile of an already sorted, non-empty slice.
fn percentile(sorted: &[Duration], p: usize) -> Duration {
    let rank = (p * sorted.len()).div_ceil(100).max(1);
    sorted[rank - 1]
}

impl fmt::Display for Summary {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "n={} min={} median={} p90={} max={}",
            self.count,
            ms(self.min),
            ms(self.median),
            ms(self.p90),
            ms(self.max)
        )
    }
}

pub fn ms(d: Duration) -> String {
    format!("{:.1} ms", d.as_secs_f64() * 1000.0)
}

/// CPU usage of `cpu_time` over `wall` as a percentage of the whole machine.
pub fn cpu_percent(cpu_time: Duration, wall: Duration, logical_cpus: usize) -> f64 {
    if wall.is_zero() || logical_cpus == 0 {
        return 0.0;
    }
    cpu_time.as_secs_f64() / (wall.as_secs_f64() * logical_cpus as f64) * 100.0
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ms_list(values: &[u64]) -> Vec<Duration> {
        values.iter().map(|&v| Duration::from_millis(v)).collect()
    }

    #[test]
    fn empty_has_no_summary() {
        assert_eq!(Summary::of(&[]), None);
    }

    #[test]
    fn single_sample() {
        let s = Summary::of(&ms_list(&[42])).unwrap();
        assert_eq!(
            (s.min, s.median, s.p90, s.max),
            (
                Duration::from_millis(42),
                Duration::from_millis(42),
                Duration::from_millis(42),
                Duration::from_millis(42)
            )
        );
    }

    #[test]
    fn unsorted_input_ten_samples() {
        let s = Summary::of(&ms_list(&[10, 1, 9, 2, 8, 3, 7, 4, 6, 5])).unwrap();
        assert_eq!(s.count, 10);
        assert_eq!(s.min, Duration::from_millis(1));
        assert_eq!(s.median, Duration::from_millis(5)); // nearest rank 5
        assert_eq!(s.p90, Duration::from_millis(9)); // nearest rank 9
        assert_eq!(s.max, Duration::from_millis(10));
    }

    #[test]
    fn p90_of_small_set_is_the_max() {
        let s = Summary::of(&ms_list(&[3, 1, 2])).unwrap();
        assert_eq!(s.p90, Duration::from_millis(3));
        assert_eq!(s.median, Duration::from_millis(2));
    }

    #[test]
    fn cpu_percent_of_machine() {
        // 0.6 s of CPU over 60 s on 4 logical CPUs = 0.25 %.
        let p = cpu_percent(Duration::from_millis(600), Duration::from_secs(60), 4);
        assert!((p - 0.25).abs() < 1e-9, "{p}");
        assert_eq!(cpu_percent(Duration::from_secs(1), Duration::ZERO, 4), 0.0);
    }

    #[test]
    fn display_is_in_milliseconds() {
        let s = Summary::of(&ms_list(&[1, 2])).unwrap();
        assert_eq!(
            s.to_string(),
            "n=2 min=1.0 ms median=1.0 ms p90=2.0 ms max=2.0 ms"
        );
    }
}
