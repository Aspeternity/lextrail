//! LexTrail text capture layer with per-platform adapters (ARCHITECTURE.md 4, D007, D011).
//!
//! Use:
//! 1. When the hotkey fires, call [`TextCapture::snapshot`] *before* showing the popup.
//! 2. Check the application blacklist against [`CaptureTarget::window`] (M5).
//! 3. Call [`TextCapture::capture`] on a background thread; it blocks up to about the
//!    time budget.
//!
//! "Nothing to capture" ([`CaptureOutcome::Unavailable`]) is a normal result the popup
//! explains to the user; [`CaptureError`] means something is broken and is logged.

mod pipeline;
mod text_offsets;
#[cfg(windows)]
mod windows;

use std::fmt;
use std::ops::Range;
use std::time::Duration;

pub use lextrail_core::lookup::CaptureMethod;
pub use pipeline::{CaptureStep, Pipeline};
pub use text_offsets::utf16_range_to_utf8;
#[cfg(windows)]
pub use windows::WindowsCapture;

/// The capture adapter for one platform (D007).
pub trait TextCapture: Send + Sync {
    /// Records the foreground window and focus. Cheap (no cross-process calls that can
    /// hang); call it the moment the hotkey fires, before the popup appears.
    fn snapshot(&self) -> Result<CaptureTarget, CaptureError>;

    /// Reads the selected text and its context from `target`. Blocks for up to about the
    /// time budget; never call it on the UI thread.
    fn capture(&self, target: &CaptureTarget) -> Result<CaptureOutcome, CaptureError> {
        self.capture_traced(target).0
    }

    /// [`TextCapture::capture`] plus what each method did and how long it took, for
    /// diagnostics. The trace holds no captured text.
    fn capture_traced(
        &self,
        target: &CaptureTarget,
    ) -> (Result<CaptureOutcome, CaptureError>, CaptureTrace);

    /// Reads only the context around `selected_text` (already known, e.g. from a double
    /// Ctrl+C, D012). `Ok(None)` when no context is available.
    fn capture_context(
        &self,
        target: &CaptureTarget,
        selected_text: &str,
    ) -> Result<Option<RawContext>, CaptureError>;
}

/// The foreground window and focus at the moment the hotkey fired.
#[derive(Debug, Clone)]
pub struct CaptureTarget {
    /// `None` when there was no foreground window (e.g. the desktop had focus).
    pub window: Option<WindowInfo>,
    pub(crate) platform: PlatformTarget,
}

impl CaptureTarget {
    /// The platform handle of the foreground window (`HWND` on Windows), 0 if none.
    /// Lets the caller ignore events about the window it is reading from.
    pub fn native_window(&self) -> isize {
        self.platform.hwnd
    }

    /// A target with no platform handles, for tests of code that consumes targets.
    /// Platform adapters treat it as having no focused element.
    pub fn for_tests(window: Option<WindowInfo>) -> Self {
        Self {
            window,
            platform: PlatformTarget::default(),
        }
    }
}

/// Adapter for platforms without capture support yet: always [`Unavailable::Unsupported`].
#[derive(Debug, Clone, Copy, Default)]
pub struct UnsupportedCapture;

impl TextCapture for UnsupportedCapture {
    fn snapshot(&self) -> Result<CaptureTarget, CaptureError> {
        Ok(CaptureTarget {
            window: None,
            platform: PlatformTarget::default(),
        })
    }

    fn capture_traced(
        &self,
        _target: &CaptureTarget,
    ) -> (Result<CaptureOutcome, CaptureError>, CaptureTrace) {
        (
            Ok(CaptureOutcome::Unavailable(Unavailable::Unsupported)),
            CaptureTrace::default(),
        )
    }

    fn capture_context(
        &self,
        _target: &CaptureTarget,
        _selected_text: &str,
    ) -> Result<Option<RawContext>, CaptureError> {
        Ok(None)
    }
}

/// Platform-specific handles captured by [`TextCapture::snapshot`].
#[derive(Debug, Clone, Default)]
pub(crate) struct PlatformTarget {
    /// Raw `HWND` values: top-level window, focused window and caret window of the
    /// foreground thread (`GetGUIThreadInfo`). 0 = none.
    pub hwnd: isize,
    #[cfg_attr(not(windows), allow(dead_code))]
    pub hwnd_focus: isize,
    #[cfg_attr(not(windows), allow(dead_code))]
    pub hwnd_caret: isize,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WindowInfo {
    pub process_id: u32,
    /// Executable file name, e.g. `chrome.exe`. May be stored with a lookup.
    pub process_name: Option<String>,
    /// Window class, e.g. to recognise terminals (Ctrl+Insert instead of Ctrl+C).
    pub class_name: Option<String>,
    /// Window title. Kept in memory only; never written to the database (ARCHITECTURE.md 6.3).
    pub title: Option<String>,
    pub integrity: Integrity,
}

impl WindowInfo {
    /// A plain window of another process, for tests.
    pub fn for_tests(process_name: &str) -> Self {
        Self {
            process_id: u32::MAX,
            process_name: Some(process_name.to_owned()),
            class_name: None,
            title: None,
            integrity: Integrity::SameOrLower,
        }
    }
}

/// The target process's privilege level compared with LexTrail's (UIPI).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Integrity {
    SameOrLower,
    /// Runs elevated (e.g. as administrator): UIA and simulated keys are blocked.
    Higher,
    /// Could not be determined; capture is attempted.
    Unknown,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CaptureOutcome {
    Captured(Selection),
    Unavailable(Unavailable),
}

/// Why nothing was captured. All of these are normal situations.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Unavailable {
    /// Nothing is selected.
    NoSelection,
    /// The target runs with higher privileges than LexTrail (UIPI). A double Ctrl+C
    /// still works, because the target copies the text itself.
    Elevated,
    NoForegroundWindow,
    /// No capture method can read text here (custom-drawn UI, LexTrail itself, ...).
    Unsupported,
    /// Every method ran out of time.
    TimedOut,
}

/// Something went wrong while capturing. Platform errors are converted inside the
/// adapter, so this type is the same on every platform (D003, D007).
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum CaptureError {
    /// An OS call failed. `code` is the platform's error code (an `HRESULT` on Windows).
    #[error("{operation} failed: {message} (0x{code:08x})")]
    Platform {
        operation: &'static str,
        code: u32,
        message: String,
    },
    /// The background worker could not be started or has stopped.
    #[error("capture worker unavailable: {0}")]
    WorkerUnavailable(String),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Selection {
    pub text: String,
    pub context: Option<RawContext>,
    /// Reserved for the browser extension; always `None` for now.
    pub url: Option<String>,
    pub method: CaptureMethod,
}

/// The paragraph or line around the selection, as captured. Cutting out the sentence is
/// done in `lextrail_core::text::sentence_around`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RawContext {
    pub text: String,
    /// Where the selection is in `text`: UTF-8 **byte** offsets that fall on character
    /// boundaries. Adapters convert from their native units (UTF-16 on Windows) with
    /// [`utf16_range_to_utf8`]. `None` when unknown.
    pub selection: Option<Range<usize>>,
}

/// Where a capture step found the element it read from (Windows UIA: the UIA focus, or
/// the `GetGUIThreadInfo` focus / caret window, or the foreground window).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ElementSource {
    UiaFocus,
    HwndFocus,
    HwndCaret,
    ForegroundWindow,
}

impl ElementSource {
    pub fn label(self) -> &'static str {
        match self {
            Self::UiaFocus => "uia_focus",
            Self::HwndFocus => "hwnd_focus",
            Self::HwndCaret => "hwnd_caret",
            Self::ForegroundWindow => "foreground_window",
        }
    }
}

/// What one [`CaptureStep`] returned.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StepOutput {
    pub result: Result<CaptureOutcome, CaptureError>,
    /// The element the result came from, when the step knows it.
    pub element: Option<ElementSource>,
}

impl From<Result<CaptureOutcome, CaptureError>> for StepOutput {
    fn from(result: Result<CaptureOutcome, CaptureError>) -> Self {
        Self {
            result,
            element: None,
        }
    }
}

/// Diagnostics for one capture: each step that ran, in order. Contains no captured text.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct CaptureTrace {
    pub steps: Vec<StepTrace>,
    /// Set when a pre-check decided the result without running any step.
    pub precheck: Option<Unavailable>,
    pub total: Duration,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StepTrace {
    pub step: &'static str,
    pub result: &'static str,
    pub elapsed: Duration,
    pub element: Option<ElementSource>,
}

/// A short, text-free label for a capture result: `captured`, `no_selection`, ..., `error`.
pub fn result_label(result: &Result<CaptureOutcome, CaptureError>) -> &'static str {
    match result {
        Ok(CaptureOutcome::Captured(_)) => "captured",
        Ok(CaptureOutcome::Unavailable(u)) => u.label(),
        Err(_) => "error",
    }
}

impl Unavailable {
    pub fn label(self) -> &'static str {
        match self {
            Self::NoSelection => "no_selection",
            Self::Elevated => "elevated",
            Self::NoForegroundWindow => "no_foreground_window",
            Self::Unsupported => "unsupported",
            Self::TimedOut => "timed_out",
        }
    }
}

impl fmt::Display for CaptureTrace {
    /// e.g. `uia=captured 21.3ms (uia_focus); total 21.4ms` or `precheck=elevated; total 0.0ms`.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let ms = |d: Duration| d.as_secs_f64() * 1000.0;
        if let Some(u) = self.precheck {
            write!(f, "precheck={}; ", u.label())?;
        }
        for step in &self.steps {
            write!(f, "{}={} {:.1}ms", step.step, step.result, ms(step.elapsed))?;
            if let Some(element) = step.element {
                write!(f, " ({})", element.label())?;
            }
            f.write_str("; ")?;
        }
        write!(f, "total {:.1}ms", ms(self.total))
    }
}

impl fmt::Display for Unavailable {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::NoSelection => "no selection",
            Self::Elevated => "target runs elevated",
            Self::NoForegroundWindow => "no foreground window",
            Self::Unsupported => "unsupported target",
            Self::TimedOut => "timed out",
        })
    }
}
