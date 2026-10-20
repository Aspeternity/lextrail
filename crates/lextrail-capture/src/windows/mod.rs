//! Windows capture adapter: UI Automation, then the clipboard fallback (D011). OCR is a
//! separate entry point (M5).

mod clipboard;
mod double_copy;
mod uia;
mod window;

use std::sync::Arc;
use std::time::{Duration, Instant};

use crate::{
    CaptureError, CaptureOutcome, CaptureTarget, CaptureTrace, Pipeline, Progress, RawContext,
    StepTrace, TextCapture, Unavailable, result_label,
};
pub use double_copy::DoubleCopyListener;
use uia::UiaStep;

pub struct WindowsCapture {
    uia: Arc<UiaStep>,
    pipeline: Pipeline,
}

impl WindowsCapture {
    pub fn new() -> Self {
        Self::with_uia_budget(uia::BUDGET)
    }

    pub fn with_uia_budget(budget: Duration) -> Self {
        let uia = Arc::new(UiaStep::new(budget));
        let clipboard: Box<dyn crate::CaptureStep> = Box::new(clipboard::ClipboardStep::new());
        // Debug builds only: LEXTRAIL_DEBUG_CLIPBOARD_ONLY=1 skips UI Automation, to test
        // the clipboard fallback in programs where UIA works.
        #[cfg(debug_assertions)]
        if std::env::var_os("LEXTRAIL_DEBUG_CLIPBOARD_ONLY").is_some() {
            return Self {
                uia,
                pipeline: Pipeline::new(vec![clipboard]),
            };
        }
        let pipeline = Pipeline::new(vec![Box::new(Arc::clone(&uia)), clipboard]);
        Self { uia, pipeline }
    }
}

/// The desktop window classes (wallpaper and icons).
fn is_desktop_class(class: &str) -> bool {
    class == "Progman" || class == "WorkerW"
}

impl Default for WindowsCapture {
    fn default() -> Self {
        Self::new()
    }
}

impl TextCapture for WindowsCapture {
    fn snapshot(&self) -> Result<CaptureTarget, CaptureError> {
        let target = window::snapshot();
        self.uia.warm_up();
        Ok(target)
    }

    fn capture_traced(
        &self,
        target: &CaptureTarget,
        progress: Progress<'_>,
    ) -> (Result<CaptureOutcome, CaptureError>, CaptureTrace) {
        // The desktop has nothing to select: say so rather than "unsupported".
        if target
            .window
            .as_ref()
            .and_then(|w| w.class_name.as_deref())
            .is_some_and(is_desktop_class)
        {
            let trace = CaptureTrace {
                precheck: Some(Unavailable::NoSelection),
                ..CaptureTrace::default()
            };
            return (
                Ok(CaptureOutcome::Unavailable(Unavailable::NoSelection)),
                trace,
            );
        }
        self.pipeline.run_traced(target, progress)
    }

    fn capture_context_traced(
        &self,
        target: &CaptureTarget,
        selected_text: &str,
    ) -> (Result<Option<RawContext>, CaptureError>, CaptureTrace) {
        let start = Instant::now();
        let output = self.uia.context(target, selected_text);
        let elapsed = start.elapsed();
        let trace = CaptureTrace {
            steps: vec![StepTrace {
                step: "uia_context",
                result: result_label(&output.result),
                elapsed,
                element: output.element,
                attempts: output.attempts,
                note: output.note,
                key_wait: None,
            }],
            precheck: None,
            total: elapsed,
        };
        let result = match output.result {
            Ok(CaptureOutcome::Captured(selection)) => Ok(selection.context),
            Ok(CaptureOutcome::Unavailable(_)) => Ok(None),
            Err(e) => Err(e),
        };
        (result, trace)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::WindowInfo;

    #[test]
    fn desktop_gives_no_selection_without_running_steps() {
        assert!(is_desktop_class("Progman"));
        assert!(is_desktop_class("WorkerW"));
        assert!(!is_desktop_class("Notepad"));

        let mut window = WindowInfo::for_tests("explorer.exe");
        window.class_name = Some("Progman".into());
        let (result, trace) =
            WindowsCapture::new().capture_traced(&CaptureTarget::for_tests(Some(window)), &|_| {});
        assert_eq!(
            result,
            Ok(CaptureOutcome::Unavailable(Unavailable::NoSelection))
        );
        assert_eq!(trace.precheck, Some(Unavailable::NoSelection));
        assert!(trace.steps.is_empty());
    }
}
