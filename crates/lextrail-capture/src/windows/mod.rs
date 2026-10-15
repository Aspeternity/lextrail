//! Windows capture adapter: UI Automation now; clipboard fallback and OCR follow (D011).

mod uia;
mod window;

use std::sync::Arc;
use std::time::Duration;

use crate::{CaptureError, CaptureOutcome, CaptureTarget, Pipeline, RawContext, TextCapture};
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
        // The clipboard fallback is added after UIA in its own task.
        let pipeline = Pipeline::new(vec![Box::new(Arc::clone(&uia))]);
        Self { uia, pipeline }
    }
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

    fn capture(&self, target: &CaptureTarget) -> Result<CaptureOutcome, CaptureError> {
        self.pipeline.run(target)
    }

    fn capture_context(
        &self,
        _target: &CaptureTarget,
        _selected_text: &str,
    ) -> Result<Option<RawContext>, CaptureError> {
        // Implemented together with the double Ctrl+C trigger (D012).
        Ok(None)
    }
}
