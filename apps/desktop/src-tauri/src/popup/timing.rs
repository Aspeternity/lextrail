//! Per-lookup timings for the benchmark. Enabled only when the environment variable
//! [`ENV`] names a file; one line per lookup that reached the focused popup is appended.
//! Lines contain numbers only, never captured text (D032).
//!
//! ```text
//! id=3 capture_ms=21.3 window_ready_ms=4.8 shown_ms=22.1 focused_ms=75.0 focused=true
//! ```
//! `capture_ms` is how long the capture itself took; the other times are measured from
//! the hotkey handler. A step that did not happen is written as `-`.

use std::fs::{File, OpenOptions};
use std::io::Write;
use std::sync::Mutex;
use std::time::{Duration, Instant};

pub const ENV: &str = "LEXTRAIL_BENCH_TIMINGS";

pub struct Timings {
    file: Mutex<File>,
    current: Mutex<Option<Record>>,
}

struct Record {
    id: u64,
    start: Instant,
    capture: Option<Duration>,
    window_ready: Option<Duration>,
    shown: Option<Duration>,
}

impl Timings {
    pub fn from_env() -> Option<Self> {
        let path = std::env::var_os(ENV)?;
        match OpenOptions::new().create(true).append(true).open(&path) {
            Ok(file) => Some(Self {
                file: Mutex::new(file),
                current: Mutex::new(None),
            }),
            Err(e) => {
                eprintln!("cannot open {ENV} file: {e}");
                None
            }
        }
    }

    /// The hotkey was pressed for lookup `id`.
    pub fn start(&self, id: u64) {
        *lock(&self.current) = Some(Record {
            id,
            start: Instant::now(),
            capture: None,
            window_ready: None,
            shown: None,
        });
    }

    pub fn capture(&self, id: u64, took: Duration) {
        if let Some(r) = lock(&self.current).as_mut()
            && r.id == id
        {
            r.capture = Some(took);
        }
    }

    pub fn window_ready(&self) {
        if let Some(r) = lock(&self.current).as_mut() {
            r.window_ready.get_or_insert(r.start.elapsed());
        }
    }

    pub fn shown(&self) {
        if let Some(r) = lock(&self.current).as_mut() {
            r.shown.get_or_insert(r.start.elapsed());
        }
    }

    /// Activation finished: writes the line for the current lookup.
    pub fn focused(&self, ok: bool) {
        let Some(r) = lock(&self.current).take() else {
            return;
        };
        let line = format_line(
            r.id,
            r.capture,
            r.window_ready,
            r.shown,
            Some(r.start.elapsed()),
            ok,
        );
        if let Err(e) = writeln!(lock(&self.file), "{line}") {
            eprintln!("cannot write {ENV} file: {e}");
        }
    }
}

fn format_line(
    id: u64,
    capture: Option<Duration>,
    window_ready: Option<Duration>,
    shown: Option<Duration>,
    focused: Option<Duration>,
    ok: bool,
) -> String {
    let ms = |d: Option<Duration>| {
        d.map_or_else(
            || "-".to_owned(),
            |d| format!("{:.1}", d.as_secs_f64() * 1000.0),
        )
    };
    format!(
        "id={id} capture_ms={} window_ready_ms={} shown_ms={} focused_ms={} focused={ok}",
        ms(capture),
        ms(window_ready),
        ms(shown),
        ms(focused)
    )
}

fn lock<T>(mutex: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(|e| e.into_inner())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn line_format() {
        let ms = Duration::from_micros;
        assert_eq!(
            format_line(
                3,
                Some(ms(21_340)),
                Some(ms(4_800)),
                None,
                Some(ms(75_000)),
                true
            ),
            "id=3 capture_ms=21.3 window_ready_ms=4.8 shown_ms=- focused_ms=75.0 focused=true"
        );
    }
}
