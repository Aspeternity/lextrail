//! UI Automation capture on a dedicated worker thread.
//!
//! Microsoft's guidance for UIA clients: make the calls from a separate thread that owns
//! no windows and is a COM multithreaded-apartment (MTA) thread
//! (<https://learn.microsoft.com/windows/win32/winauto/uiauto-threading>). The worker is
//! such a thread; it is started on the first hotkey press, not at app start.
//!
//! A cross-process UIA call can hang. The caller only waits for the time budget; a worker
//! still busy after [`STUCK_AFTER`] is abandoned (left to finish on its own) and replaced.
//! At most [`MAX_ABANDONED`] abandoned workers may exist; beyond that UIA is skipped
//! until one of them finishes.

use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::mpsc::{self, Receiver, RecvTimeoutError, Sender, SyncSender};
use std::sync::{Arc, Mutex, MutexGuard};
use std::thread;
use std::time::{Duration, Instant};

use windows::Win32::Foundation::{E_ACCESSDENIED, RPC_E_DISCONNECTED};
use windows::Win32::System::Com::{
    CLSCTX_INPROC_SERVER, COINIT_MULTITHREADED, CoCreateInstance, CoInitializeEx, CoUninitialize,
};
use windows::Win32::UI::Accessibility::{
    CUIAutomation8, IUIAutomation2, IUIAutomationElement, IUIAutomationTextPattern,
    IUIAutomationTextRange, TextPatternRangeEndpoint_End, TextPatternRangeEndpoint_Start,
    TextUnit_Line, TextUnit_Paragraph, UIA_E_ELEMENTNOTAVAILABLE, UIA_E_NOTSUPPORTED,
    UIA_E_TIMEOUT, UIA_TextPatternId,
};

use super::window::hwnd;
use crate::double_copy::selection_holds;
use crate::{
    CaptureError, CaptureMethod, CaptureOutcome, CaptureStep, CaptureTarget, ElementSource,
    Progress, RawContext, Selection, StepContext, StepOutput, Unavailable, utf16_range_to_utf8,
};

/// Time the caller waits for UIA before falling back (150–200 ms, D011).
pub const BUDGET: Duration = Duration::from_millis(180);
/// Extra wait on the very first request, while the worker sets up COM and UIA.
const STARTUP_ALLOWANCE: Duration = Duration::from_millis(500);
/// A worker busy for this long is considered stuck and replaced.
const STUCK_AFTER: Duration = Duration::from_secs(5);
/// Abandoned (stuck) workers allowed at once; more disables UIA until one finishes.
const MAX_ABANDONED: usize = 2;

const MAX_SELECTION_UTF16: i32 = 4000;
/// A "paragraph" longer than this (some apps return the whole document) is skipped in
/// favour of the line.
const MAX_CONTEXT_UTF16: i32 = 8000;
/// How far up the tree to look for an element with a text pattern.
const MAX_PARENT_STEPS: usize = 8;
/// Chromium windows: pause between attempts while the accessibility tree is being built.
const CHROMIUM_RETRY_INTERVAL: Duration = Duration::from_millis(30);
/// Time kept free at the end of the budget, so a retry never makes the caller give up.
const RETRY_MARGIN: Duration = Duration::from_millis(20);

const RPC_S_SERVER_UNAVAILABLE: u32 = 0x8007_06ba;

pub struct UiaStep {
    budget: Duration,
    worker: Mutex<Option<Worker>>,
    abandoned: Arc<AtomicUsize>,
}

struct Worker {
    jobs: Sender<Job>,
    /// When the current job was handed over; `None` while idle.
    busy_since: Arc<Mutex<Option<Instant>>>,
    ready: Arc<AtomicBool>,
    abandoned: Arc<AtomicBool>,
}

struct Job {
    /// When the caller handed the job over; retries stay within the budget from here.
    queued_at: Instant,
    target: CaptureTarget,
    kind: JobKind,
    reply: SyncSender<StepOutput>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum JobKind {
    /// Read the selection and its context (hotkey).
    Selection,
    /// The selected text is known (double Ctrl+C): read only the context, and only if
    /// the selection holds this text. The result is captured with this text.
    Context(String),
}

impl UiaStep {
    pub fn new(budget: Duration) -> Self {
        Self {
            budget,
            worker: Mutex::new(None),
            abandoned: Arc::new(AtomicUsize::new(0)),
        }
    }

    /// Starts the worker if needed, so COM setup overlaps with showing the popup.
    pub fn warm_up(&self) {
        let mut worker = lock(&self.worker);
        self.ensure_worker(&mut worker);
    }

    fn ensure_worker<'a>(&self, slot: &'a mut Option<Worker>) -> Option<&'a Worker> {
        if let Some(w) = slot.as_ref()
            && lock(&w.busy_since).is_some_and(|since| since.elapsed() >= STUCK_AFTER)
        {
            w.abandoned.store(true, Ordering::SeqCst);
            self.abandoned.fetch_add(1, Ordering::SeqCst);
            // Dropping the sender lets the thread exit once its stuck call returns.
            *slot = None;
        }
        if slot.is_none() {
            if self.abandoned.load(Ordering::SeqCst) >= MAX_ABANDONED {
                return None;
            }
            match spawn(self.budget, Arc::clone(&self.abandoned)) {
                Ok(w) => *slot = Some(w),
                Err(e) => {
                    eprintln!("capture: {e}");
                    return None;
                }
            }
        }
        slot.as_ref()
    }

    #[cfg(test)]
    fn abandoned_workers(&self) -> usize {
        self.abandoned.load(Ordering::SeqCst)
    }
}

impl UiaStep {
    /// The context around `text`, known to be selected (double Ctrl+C), on the same
    /// worker and with the same budget as a hotkey capture: `Captured` with `text` and
    /// the context found, or `Unavailable` (with [`crate::NOTE_CONTEXT_MISMATCH`] when the
    /// selection held other text).
    pub fn context(&self, target: &CaptureTarget, text: &str) -> StepOutput {
        self.run(target, JobKind::Context(text.to_owned()))
    }

    fn run(&self, target: &CaptureTarget, kind: JobKind) -> StepOutput {
        let unavailable = |u| StepOutput::from(Ok(CaptureOutcome::Unavailable(u)));
        if target.platform.hwnd == 0 {
            return unavailable(Unavailable::Unsupported);
        }
        let (jobs, busy, wait) = {
            let mut slot = lock(&self.worker);
            // UIA disabled (too many stuck workers): let the next method try.
            let Some(w) = self.ensure_worker(&mut slot) else {
                return unavailable(Unavailable::TimedOut);
            };
            let mut busy = lock(&w.busy_since);
            if busy.is_some() {
                // The previous request is still running (slow target): skip UIA.
                return unavailable(Unavailable::TimedOut);
            }
            *busy = Some(Instant::now());
            let wait = if w.ready.load(Ordering::SeqCst) {
                self.budget
            } else {
                self.budget + STARTUP_ALLOWANCE
            };
            (w.jobs.clone(), Arc::clone(&w.busy_since), wait)
        };

        let (reply, result) = mpsc::sync_channel(1);
        let job = Job {
            queued_at: Instant::now(),
            target: target.clone(),
            kind,
            reply,
        };
        if jobs.send(job).is_err() {
            *lock(&busy) = None;
            let mut slot = lock(&self.worker);
            if slot
                .as_ref()
                .is_some_and(|w| Arc::ptr_eq(&w.busy_since, &busy))
            {
                *slot = None;
            }
            return Err(worker_stopped()).into();
        }
        match result.recv_timeout(wait) {
            Ok(output) => output,
            Err(RecvTimeoutError::Timeout) => unavailable(Unavailable::TimedOut),
            Err(RecvTimeoutError::Disconnected) => Err(worker_stopped()).into(),
        }
    }
}

impl CaptureStep for UiaStep {
    fn name(&self) -> &'static str {
        "uia"
    }

    fn try_capture(
        &self,
        target: &CaptureTarget,
        _context: &StepContext,
        _progress: Progress<'_>,
    ) -> StepOutput {
        self.run(target, JobKind::Selection)
    }
}

fn worker_stopped() -> CaptureError {
    CaptureError::WorkerUnavailable("UI Automation worker stopped".into())
}

fn spawn(budget: Duration, abandoned_count: Arc<AtomicUsize>) -> Result<Worker, CaptureError> {
    let (jobs, receiver) = mpsc::channel();
    let worker = Worker {
        jobs,
        busy_since: Arc::new(Mutex::new(None)),
        ready: Arc::new(AtomicBool::new(false)),
        abandoned: Arc::new(AtomicBool::new(false)),
    };
    let busy = Arc::clone(&worker.busy_since);
    let ready = Arc::clone(&worker.ready);
    let abandoned = Arc::clone(&worker.abandoned);
    thread::Builder::new()
        .name("lextrail-uia".into())
        .spawn(move || {
            run_worker(receiver, budget, &busy, &ready);
            if abandoned.load(Ordering::SeqCst) {
                abandoned_count.fetch_sub(1, Ordering::SeqCst);
            }
        })
        .map_err(|e| CaptureError::WorkerUnavailable(e.to_string()))?;
    Ok(worker)
}

fn run_worker(
    jobs: Receiver<Job>,
    budget: Duration,
    busy: &Mutex<Option<Instant>>,
    ready: &AtomicBool,
) {
    // SAFETY: called once on this thread, balanced by CoUninitialize below.
    let init = unsafe { CoInitializeEx(None, COINIT_MULTITHREADED) };
    let automation = init
        .ok()
        .map_err(|e| platform("CoInitializeEx", &e))
        .and_then(|()| create_automation(budget));
    ready.store(true, Ordering::SeqCst);

    for job in jobs {
        let output = match &automation {
            Ok(automation) => read_with_retry(automation, &job, budget),
            Err(e) => Err(e.clone()).into(),
        };
        *lock(busy) = None;
        // The caller may have stopped waiting; that is fine.
        let _ = job.reply.send(output);
    }

    drop(automation);
    if init.is_ok() {
        // SAFETY: balances the successful CoInitializeEx above; all COM objects are gone.
        unsafe { CoUninitialize() };
    }
}

fn create_automation(budget: Duration) -> Result<IUIAutomation2, CaptureError> {
    let millis = u32::try_from(budget.as_millis()).unwrap_or(u32::MAX);
    // SAFETY: COM is initialized on this thread.
    unsafe {
        let automation: IUIAutomation2 =
            CoCreateInstance(&CUIAutomation8, None, CLSCTX_INPROC_SERVER)
                .map_err(|e| platform("CoCreateInstance(CUIAutomation8)", &e))?;
        // Make hung providers fail fast instead of blocking for the 2 s / 20 s defaults.
        automation
            .SetConnectionTimeout(millis)
            .and_then(|()| automation.SetTransactionTimeout(millis))
            .map_err(|e| platform("IUIAutomation2 timeouts", &e))?;
        Ok(automation)
    }
}

fn platform(operation: &'static str, e: &windows::core::Error) -> CaptureError {
    CaptureError::Platform {
        operation,
        code: e.code().0 as u32,
        message: e.message(),
    }
}

/// Maps errors that describe the target (not a fault in LexTrail) to outcomes.
fn classify(e: &windows::core::Error) -> Result<CaptureOutcome, CaptureError> {
    let unavailable = |u| Ok(CaptureOutcome::Unavailable(u));
    match e.code().0 as u32 {
        code if code == E_ACCESSDENIED.0 as u32 => unavailable(Unavailable::Elevated),
        UIA_E_TIMEOUT => unavailable(Unavailable::TimedOut),
        UIA_E_ELEMENTNOTAVAILABLE | UIA_E_NOTSUPPORTED | RPC_S_SERVER_UNAVAILABLE => {
            unavailable(Unavailable::Unsupported)
        }
        code if code == RPC_E_DISCONNECTED.0 as u32 => unavailable(Unavailable::Unsupported),
        _ => Err(platform("UI Automation", e)),
    }
}

/// Chromium-based windows (Chrome, Edge, Electron apps such as VS Code) turn on their
/// accessibility tree only after they notice a UIA client, so the first request to a fresh
/// process finds no text element ("unsupported"). Asking again about 30 ms later works
/// (verified with Edge: the second attempt reads the selection). Such windows are retried
/// while the budget allows.
fn read_with_retry(automation: &IUIAutomation2, job: &Job, budget: Duration) -> StepOutput {
    let deadline = job.queued_at + budget.saturating_sub(RETRY_MARGIN);
    let mut output = read(automation, &job.target, &job.kind);
    while is_chromium(&job.target)
        && output.result == Ok(CaptureOutcome::Unavailable(Unavailable::Unsupported))
        // A note means "unsupported" was decided on purpose (e.g. a PDF), not a tree
        // that is still being built.
        && output.note.is_none()
        && Instant::now() + CHROMIUM_RETRY_INTERVAL < deadline
    {
        thread::sleep(CHROMIUM_RETRY_INTERVAL);
        let attempts = output.attempts + 1;
        output = read(automation, &job.target, &job.kind);
        output.attempts = attempts;
    }
    output
}

fn is_chromium(target: &CaptureTarget) -> bool {
    target
        .window
        .as_ref()
        .and_then(|w| w.class_name.as_deref())
        .is_some_and(crate::clipboard_rules::is_chromium_class)
}

fn read(automation: &IUIAutomation2, target: &CaptureTarget, kind: &JobKind) -> StepOutput {
    let process_id = target.window.as_ref().map_or(0, |w| w.process_id);
    let mut element = None;
    let mut note = None;
    let mut focused_class = None;
    let result = read_selection(
        automation,
        target,
        kind,
        process_id,
        &mut element,
        &mut note,
        &mut focused_class,
    )
    .or_else(|e| classify(&e));
    StepOutput {
        result,
        element,
        attempts: 1,
        note,
        focused_class,
        key_wait: None,
    }
}

/// Tries the focused element (if it belongs to the target process), then the elements of
/// the focus, caret and top-level window handles recorded by `GetGUIThreadInfo`.
/// Reports the focused element's class name for later steps (e.g. to recognise VS Code's
/// terminal, where Ctrl+C would interrupt the running program).
fn read_selection(
    automation: &IUIAutomation2,
    target: &CaptureTarget,
    kind: &JobKind,
    process_id: u32,
    used: &mut Option<ElementSource>,
    note: &mut Option<&'static str>,
    focused_class: &mut Option<String>,
) -> windows::core::Result<CaptureOutcome> {
    let p = &target.platform;
    // SAFETY: COM calls on an MTA thread with valid arguments.
    let mut candidates: Vec<(ElementSource, IUIAutomationElement)> = Vec::new();
    unsafe {
        if let Ok(focused) = automation.GetFocusedElement()
            && same_process(&focused, process_id)
        {
            *focused_class = focused
                .CurrentClassName()
                .ok()
                .map(|c| c.to_string())
                .filter(|c| !c.is_empty());
            candidates.push((ElementSource::UiaFocus, focused));
        }
        for (source, raw) in [
            (ElementSource::HwndFocus, p.hwnd_focus),
            (ElementSource::HwndCaret, p.hwnd_caret),
            (ElementSource::ForegroundWindow, p.hwnd),
        ] {
            if raw != 0
                && let Ok(element) = automation.ElementFromHandle(hwnd(raw))
            {
                candidates.push((source, element));
            }
        }
    }

    // Reports the element that produced the selection, or else the first one with a
    // text pattern (where "no selection" was decided).
    let mut found_pattern = false;
    for (source, element) in candidates {
        let Some((holder, pattern)) = text_pattern(automation, element, process_id)? else {
            continue;
        };
        if is_chromium(target) && in_chromium_pdf(automation, &holder) {
            // Edge's PDF viewer reports selection ranges shifted by a varying number of
            // characters (and paragraphs that start mid-paragraph): its text would be
            // wrong, which is worse than none. Leave those PDFs to the other methods.
            // Chrome's PDF viewer reports correct ranges and does not match this check
            // (its document root has no name), so it keeps using UIA (R1-6).
            *used = Some(source);
            *note = Some(crate::NOTE_CHROMIUM_PDF);
            return Ok(CaptureOutcome::Unavailable(Unavailable::Unsupported));
        }
        if !found_pattern {
            *used = Some(source);
            found_pattern = true;
        }
        if let Some(selection) = selection_of(&pattern)? {
            match kind {
                JobKind::Selection => {
                    *used = Some(source);
                    return Ok(CaptureOutcome::Captured(selection));
                }
                JobKind::Context(copied) if selection_holds(copied, &selection.text) => {
                    *used = Some(source);
                    *note = None;
                    return Ok(CaptureOutcome::Captured(Selection {
                        text: copied.clone(),
                        method: CaptureMethod::ClipboardDoubleCopy,
                        ..selection
                    }));
                }
                // Another element may still hold it; otherwise no context.
                JobKind::Context(_) => *note = Some(crate::NOTE_CONTEXT_MISMATCH),
            }
        }
    }
    Ok(CaptureOutcome::Unavailable(if found_pattern {
        Unavailable::NoSelection
    } else {
        Unavailable::Unsupported
    }))
}

fn same_process(element: &IUIAutomationElement, process_id: u32) -> bool {
    // SAFETY: COM call on a valid element.
    unsafe { element.CurrentProcessId() }.is_ok_and(|p| p as u32 == process_id)
}

/// The text pattern of `element` or its nearest ancestor in the same process, with the
/// element that has it.
fn text_pattern(
    automation: &IUIAutomation2,
    mut element: IUIAutomationElement,
    process_id: u32,
) -> windows::core::Result<Option<(IUIAutomationElement, IUIAutomationTextPattern)>> {
    // SAFETY: COM calls on an MTA thread with valid arguments.
    unsafe {
        let walker = automation.ControlViewWalker()?;
        for _ in 0..=MAX_PARENT_STEPS {
            if let Ok(pattern) =
                element.GetCurrentPatternAs::<IUIAutomationTextPattern>(UIA_TextPatternId)
            {
                return Ok(Some((element, pattern)));
            }
            match walker.GetParentElement(&element) {
                Ok(parent) if same_process(&parent, process_id) => element = parent,
                _ => break,
            }
        }
    }
    Ok(None)
}

/// Whether `element` is inside Edge's PDF viewer. Its document root (`RootWebArea`) is
/// named "PDF Document" (not localised), and the page around it is named after the
/// `.pdf` file. Chrome's PDF viewer has an unnamed root and is not matched.
fn in_chromium_pdf(automation: &IUIAutomation2, element: &IUIAutomationElement) -> bool {
    // SAFETY: COM calls on an MTA thread with valid arguments.
    unsafe {
        let Ok(walker) = automation.ControlViewWalker() else {
            return false;
        };
        let mut node = Some(element.clone());
        for _ in 0..4 {
            let Some(e) = node else { break };
            let id = e
                .CurrentAutomationId()
                .map(|s| s.to_string())
                .unwrap_or_default();
            let name = e.CurrentName().map(|s| s.to_string()).unwrap_or_default();
            if is_pdf_root(&id, &name) {
                return true;
            }
            node = walker.GetParentElement(&e).ok();
        }
    }
    false
}

fn is_pdf_root(automation_id: &str, name: &str) -> bool {
    automation_id == "RootWebArea"
        && (name == "PDF Document" || name.to_ascii_lowercase().ends_with(".pdf"))
}

fn selection_of(pattern: &IUIAutomationTextPattern) -> windows::core::Result<Option<Selection>> {
    // SAFETY: COM calls on an MTA thread with valid arguments.
    unsafe {
        let ranges = pattern.GetSelection()?;
        for i in 0..ranges.Length()? {
            let range = ranges.GetElement(i)?;
            let text = String::from_utf16_lossy(&range.GetText(MAX_SELECTION_UTF16)?);
            if text.trim().is_empty() {
                continue;
            }
            // Context is a bonus: failing to read it does not fail the capture.
            let context = context_of(&range, &text).unwrap_or(None);
            return Ok(Some(Selection {
                text,
                context,
                url: None,
                method: CaptureMethod::Uia,
            }));
        }
    }
    Ok(None)
}

/// The paragraph (or, if that is too long or empty, the line) around `range`.
fn context_of(
    range: &IUIAutomationTextRange,
    selected: &str,
) -> windows::core::Result<Option<RawContext>> {
    let selected_units = selected.encode_utf16().count();
    // SAFETY: COM calls on an MTA thread with valid arguments.
    unsafe {
        for unit in [TextUnit_Paragraph, TextUnit_Line] {
            let enclosing = range.Clone()?;
            enclosing.ExpandToEnclosingUnit(unit)?;
            let wide = enclosing.GetText(MAX_CONTEXT_UTF16)?;
            if wide.len() >= MAX_CONTEXT_UTF16 as usize {
                continue;
            }
            let text = String::from_utf16_lossy(&wide);
            if text.trim().is_empty() {
                continue;
            }
            // Length of the text between the start of the paragraph and the selection.
            let prefix = enclosing.Clone()?;
            prefix.MoveEndpointByRange(
                TextPatternRangeEndpoint_End,
                range,
                TextPatternRangeEndpoint_Start,
            )?;
            let start = prefix.GetText(MAX_CONTEXT_UTF16)?.len();
            // Lossy decoding keeps one UTF-16 unit per unpaired surrogate, so offsets hold.
            let selection = utf16_range_to_utf8(&text, start..start + selected_units);
            return Ok(Some(RawContext { text, selection }));
        }
    }
    Ok(None)
}

fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(|e| e.into_inner())
}

#[cfg(test)]
mod tests {
    use windows::Win32::UI::WindowsAndMessaging::GetDesktopWindow;

    use super::*;
    use crate::{PlatformTarget, WindowInfo};

    #[test]
    fn target_without_handles_is_unsupported() {
        let step = UiaStep::new(BUDGET);
        let target = CaptureTarget::for_tests(Some(WindowInfo::for_tests("x.exe")));
        assert_eq!(
            step.try_capture(&target, &StepContext::default(), &|_| {})
                .result,
            Ok(CaptureOutcome::Unavailable(Unavailable::Unsupported))
        );
    }

    #[test]
    fn worker_starts_and_answers_for_the_desktop_window() {
        let step = UiaStep::new(BUDGET);
        step.warm_up();
        // SAFETY: plain Win32 call.
        let desktop = unsafe { GetDesktopWindow() };
        let target = CaptureTarget {
            window: Some(WindowInfo::for_tests("explorer.exe")),
            platform: PlatformTarget {
                hwnd: desktop.0 as isize,
                ..Default::default()
            },
        };
        let outcome = step
            .try_capture(&target, &StepContext::default(), &|_| {})
            .result;
        assert!(
            matches!(outcome, Ok(CaptureOutcome::Unavailable(_))),
            "{outcome:?}"
        );
        assert_eq!(step.abandoned_workers(), 0);
    }

    #[test]
    fn classify_maps_target_errors_to_outcomes() {
        let err =
            |code: u32| windows::core::Error::from_hresult(windows::core::HRESULT(code as i32));
        let outcome = |code| classify(&err(code));
        assert_eq!(
            outcome(E_ACCESSDENIED.0 as u32),
            Ok(CaptureOutcome::Unavailable(Unavailable::Elevated))
        );
        assert_eq!(
            outcome(UIA_E_TIMEOUT),
            Ok(CaptureOutcome::Unavailable(Unavailable::TimedOut))
        );
        assert_eq!(
            outcome(UIA_E_ELEMENTNOTAVAILABLE),
            Ok(CaptureOutcome::Unavailable(Unavailable::Unsupported))
        );
        assert!(matches!(
            outcome(0x8000_4005),
            Err(CaptureError::Platform {
                code: 0x8000_4005,
                ..
            })
        ));
    }

    #[test]
    fn chromium_windows_are_recognised_by_class() {
        let with_class = |class: Option<&str>| {
            let mut window = WindowInfo::for_tests("app.exe");
            window.class_name = class.map(str::to_owned);
            CaptureTarget::for_tests(Some(window))
        };
        assert!(is_chromium(&with_class(Some("Chrome_WidgetWin_1"))));
        assert!(is_chromium(&with_class(Some("Chrome_WidgetWin_0"))));
        assert!(!is_chromium(&with_class(Some("Notepad"))));
        assert!(!is_chromium(&with_class(None)));
        assert!(!is_chromium(&CaptureTarget::for_tests(None)));
    }

    #[test]
    fn chromium_pdf_roots_are_recognised() {
        assert!(is_pdf_root("RootWebArea", "PDF Document"));
        assert!(is_pdf_root("RootWebArea", "1706.03762v7.pdf"));
        assert!(is_pdf_root("RootWebArea", "Report.PDF"));
        assert!(!is_pdf_root(
            "RootWebArea",
            "Attention Is All You Need - Wikipedia"
        ));
        assert!(!is_pdf_root("RootWebArea", ""));
        assert!(!is_pdf_root("", "PDF Document"));
    }
}
