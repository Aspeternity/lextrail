//! Per-lookup timings written by the app when `LEXTRAIL_BENCH_TIMINGS` is set
//! (`apps/desktop/src-tauri/src/popup/timing.rs`).

use std::time::Duration;

pub const ENV: &str = "LEXTRAIL_BENCH_TIMINGS";

/// One lookup as the app measured it. `capture` is the capture's own duration; the
/// others are measured from the app's hotkey handler.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct Timing {
    pub capture: Option<Duration>,
    pub window_ready: Option<Duration>,
    pub shown: Option<Duration>,
    pub focused: Option<Duration>,
}

/// Parses a line like
/// `id=3 capture_ms=21.3 window_ready_ms=4.8 shown_ms=22.1 focused_ms=75.0 focused=true`.
/// Unknown keys are ignored; `-` means "did not happen".
pub fn parse_line(line: &str) -> Timing {
    let mut timing = Timing::default();
    for field in line.split_whitespace() {
        let Some((key, value)) = field.split_once('=') else {
            continue;
        };
        let ms = value
            .parse::<f64>()
            .ok()
            .filter(|v| v.is_finite() && *v >= 0.0)
            .map(|v| Duration::from_secs_f64(v / 1000.0));
        match key {
            "capture_ms" => timing.capture = ms,
            "window_ready_ms" => timing.window_ready = ms,
            "shown_ms" => timing.shown = ms,
            "focused_ms" => timing.focused = ms,
            _ => {}
        }
    }
    timing
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_all_fields() {
        let t = parse_line(
            "id=3 capture_ms=21.5 window_ready_ms=4.0 shown_ms=- focused_ms=75.0 focused=true",
        );
        assert_eq!(t.capture, Some(Duration::from_micros(21_500)));
        assert_eq!(t.window_ready, Some(Duration::from_millis(4)));
        assert_eq!(t.shown, None);
        assert_eq!(t.focused, Some(Duration::from_millis(75)));
    }

    #[test]
    fn tolerates_garbage() {
        assert_eq!(parse_line(""), Timing::default());
        assert_eq!(parse_line("capture_ms=abc x y=1"), Timing::default());
        assert_eq!(parse_line("capture_ms=-5").capture, None);
    }
}
