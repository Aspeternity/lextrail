//! Manual probe: waits a few seconds (switch to a window and select text), then captures
//! the foreground window's selection and prints what came back, with timings.
//!
//! ```text
//! cargo run -p lextrail-capture --example capture_probe -- [delay_secs]
//! ```

#[cfg(windows)]
fn main() {
    use std::time::{Duration, Instant};

    use lextrail_capture::{CaptureOutcome, TextCapture, WindowsCapture};

    let delay = std::env::args()
        .nth(1)
        .and_then(|s| s.parse().ok())
        .unwrap_or(3);
    eprintln!("capturing in {delay} s: switch to a window and select some text");
    std::thread::sleep(Duration::from_secs(delay));

    let capture = WindowsCapture::new();
    let start = Instant::now();
    let target = capture.snapshot().expect("snapshot");
    let snapshot_time = start.elapsed();
    let (outcome, trace) = capture.capture_traced(&target);
    let capture_time = start.elapsed() - snapshot_time;

    println!("window: {:#?}", target.window);
    println!("snapshot: {snapshot_time:?}, capture: {capture_time:?}");
    println!("trace: {trace}");
    match outcome {
        Ok(CaptureOutcome::Captured(selection)) => {
            println!("selection: {:?} ({:?})", selection.text, selection.method);
            if let Some(context) = &selection.context {
                println!("context: {:?}", context.text);
                println!("selection range: {:?}", context.selection);
                let sentence = lextrail_core::text::sentence_around(
                    &context.text,
                    &selection.text,
                    context.selection.clone(),
                );
                println!("sentence: {sentence:?}");
            }
        }
        other => println!("result: {other:?}"),
    }
}

#[cfg(not(windows))]
fn main() {
    eprintln!("capture_probe only runs on Windows");
}
