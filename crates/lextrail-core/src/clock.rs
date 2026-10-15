//! Injectable wall-clock time, so HLC and timestamps can be tested with a fake clock.

use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

/// A source of wall-clock time in Unix milliseconds.
pub trait Clock {
    fn now_millis(&self) -> u64;
}

/// The operating system clock.
///
/// Not usable on `wasm32-unknown-unknown`; the PWA will supply its own [`Clock`].
#[derive(Debug, Clone, Copy, Default)]
pub struct SystemClock;

impl Clock for SystemClock {
    fn now_millis(&self) -> u64 {
        // A clock set before 1970 reads as 0; the HLC still stays monotonic.
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_or(0, |d| u64::try_from(d.as_millis()).unwrap_or(u64::MAX))
    }
}

/// A clock that only moves when told to. Clones share the same time.
#[derive(Debug, Clone, Default)]
pub struct ManualClock(Arc<AtomicU64>);

impl ManualClock {
    pub fn new(millis: u64) -> Self {
        Self(Arc::new(AtomicU64::new(millis)))
    }

    pub fn set(&self, millis: u64) {
        self.0.store(millis, Ordering::SeqCst);
    }

    pub fn advance(&self, millis: u64) {
        self.0.fetch_add(millis, Ordering::SeqCst);
    }
}

impl Clock for ManualClock {
    fn now_millis(&self) -> u64 {
        self.0.load(Ordering::SeqCst)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn manual_clock_clones_share_time() {
        let a = ManualClock::new(10);
        let b = a.clone();
        a.advance(5);
        assert_eq!(b.now_millis(), 15);
        b.set(3);
        assert_eq!(a.now_millis(), 3);
    }

    #[test]
    fn system_clock_is_after_2025() {
        assert!(SystemClock.now_millis() > 1_735_689_600_000);
    }
}
