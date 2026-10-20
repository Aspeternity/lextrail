//! The lookup popup: a single reusable window (D010), created on first use so the
//! tray-only state never loads a WebView.
//!
//! - [`lifecycle`]: whether the window exists and is awake (D009).
//! - [`request`]: one lookup, from hotkey to focused result; when the window appears and
//!   when it takes focus (ARCHITECTURE.md 9.3).
//!
//! This module turns both state machines' actions into window, WebView2 and capture calls.

#[cfg(windows)]
mod activation;
mod content;
mod foreground;
mod lifecycle;
mod placement;
mod request;
mod timing;
mod webview;

use std::sync::atomic::{AtomicIsize, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant};

#[cfg(debug_assertions)]
use lextrail_capture::result_label;
use lextrail_capture::{
    CaptureError, CaptureMethod, CaptureOutcome, CaptureProgress, CaptureTarget, CaptureTrace,
    DoubleCopy, Selection, TextCapture, Unavailable,
};
use lextrail_config::PopupMode;
use tauri::window::Color;
use tauri::{
    AppHandle, Emitter, Manager, PhysicalPosition, PhysicalSize, Runtime, WebviewUrl,
    WebviewWindow, WebviewWindowBuilder, WindowEvent,
};

use crate::dictionary::DictionaryService;
use crate::history::HistoryService;
use content::Content;
use lifecycle::{Action, Event, Lifecycle};
use placement::{Point, Rect, Size};
use request::Request;
use timing::Timings;

pub const LABEL: &str = "popup";

/// Logical size; real dimensions follow once the popup has content.
const WIDTH: f64 = 380.0;
const HEIGHT: f64 = 260.0;
/// Logical distance between the cursor and the popup edge.
const CURSOR_GAP: f64 = 12.0;
/// Matches the page background so the window never flashes white.
const BACKGROUND: Color = Color(255, 255, 255, 255);
/// How long to wait for the popup to get focus before reporting failure.
const ACTIVATION_CHECK: Duration = Duration::from_millis(50);

/// Shared popup state, managed by Tauri.
struct PopupState {
    lifecycle: Mutex<Lifecycle>,
    request: Mutex<Request>,
    mode: PopupMode,
    /// Cursor position of the latest show request; used when the popup is revealed.
    cursor: Mutex<Option<Point>>,
    capture: Arc<dyn TextCapture>,
    next_request: AtomicU64,
    /// Native handle of the window being read; its own foreground events are ignored.
    target_window: AtomicIsize,
    /// Benchmark timings, when `LEXTRAIL_BENCH_TIMINGS` is set.
    timings: Option<Timings>,
    /// Opened by the first lookup that captured text.
    dictionary: DictionaryService,
    /// Opened by the first lookup to record.
    history: HistoryService,
    /// The double Ctrl+C listener, when `hotkeys.double_ctrl_c` is on (D012).
    #[cfg(windows)]
    double_copy: std::sync::OnceLock<lextrail_capture::DoubleCopyListener>,
}

/// What started a lookup.
#[derive(Debug, Clone)]
pub enum Trigger {
    Hotkey,
    /// A double Ctrl+C: the copied text is known, only its context is read (D012).
    DoubleCopy(DoubleCopy),
}

/// Registers the popup state. Call once during setup. `record_history`: the
/// `history.record` setting; `double_ctrl_c`: `hotkeys.double_ctrl_c` (read once, like
/// every setting: a change takes effect at the next start).
pub fn init<R: Runtime>(
    app: &AppHandle<R>,
    mode: PopupMode,
    record_history: bool,
    double_ctrl_c: bool,
) {
    let timings = Timings::from_env();
    let data_dir = crate::paths::data_dir(app, timings.is_some());
    app.manage(PopupState {
        lifecycle: Mutex::new(Lifecycle::new()),
        request: Mutex::new(Request::new()),
        mode,
        cursor: Mutex::new(None),
        capture: platform_capture(),
        next_request: AtomicU64::new(0),
        target_window: AtomicIsize::new(0),
        timings,
        history: HistoryService::new(data_dir, record_history),
        dictionary: DictionaryService::new(
            app.path()
                .resource_dir()
                .ok()
                .map(|dir| dir.join(lextrail_dict::CORE_DICT_FILE_NAME)),
        ),
        #[cfg(windows)]
        double_copy: std::sync::OnceLock::new(),
    });
    if double_ctrl_c {
        start_double_copy(app);
    }
    foreground::init({
        let app = app.clone();
        move |window| {
            let state = app.state::<PopupState>();
            if window != state.target_window.load(Ordering::SeqCst) {
                dispatch_request(&app, request::Event::ForegroundChanged);
            }
        }
    });
}

/// Starts the double Ctrl+C listener; each double copy starts a lookup on the main
/// thread, like a hotkey press.
#[cfg(windows)]
fn start_double_copy<R: Runtime>(app: &AppHandle<R>) {
    let handle = app.clone();
    let started = lextrail_capture::DoubleCopyListener::start(move |double| {
        let pressed = Instant::now();
        // Without the text; tools/e2e/popup-e2e.ps1 counts these.
        #[cfg(debug_assertions)]
        eprintln!("double copy: lookup triggered");
        let app = handle.clone();
        let _ = handle.run_on_main_thread(move || {
            if let Err(e) = lookup(&app, pressed, Trigger::DoubleCopy(double)) {
                eprintln!("cannot start lookup: {e}");
            }
        });
    });
    match started {
        Ok(listener) => {
            let _ = app.state::<PopupState>().double_copy.set(listener);
        }
        Err(e) => eprintln!("double Ctrl+C is off: the clipboard listener failed ({e})"),
    }
}

#[cfg(not(windows))]
fn start_double_copy<R: Runtime>(_app: &AppHandle<R>) {}

/// Tells the double Ctrl+C listener that hotkey capture `id` starts or ends, so that
/// its clipboard fallback's updates are not taken for a double copy.
fn own_copy<R: Runtime>(app: &AppHandle<R>, id: u64, started: bool) {
    #[cfg(windows)]
    if let Some(listener) = app.state::<PopupState>().double_copy.get() {
        if started {
            listener.own_copy_started(id);
        } else {
            listener.own_copy_finished(id);
        }
    }
    #[cfg(not(windows))]
    let _ = (app, id, started);
}

/// Runs the capture a trigger calls for: the whole pipeline for the hotkey; for a double
/// Ctrl+C the copied text, with the context UI Automation finds around it (D012). The
/// bool: the text came through the clipboard from a Chromium PDF viewer.
fn run_capture<R: Runtime>(
    app: &AppHandle<R>,
    id: u64,
    capture: &dyn TextCapture,
    target: Result<CaptureTarget, CaptureError>,
    trigger: Trigger,
) -> (Result<CaptureOutcome, CaptureError>, CaptureTrace, bool) {
    let target = match target {
        Ok(t) => t,
        Err(e) => return (Err(e), CaptureTrace::default(), false),
    };
    match trigger {
        Trigger::Hotkey => {
            let progress = |p: CaptureProgress| {
                let event = match p {
                    CaptureProgress::WaitingForKeys => request::Event::WaitingForKeys { id },
                    CaptureProgress::KeysReleased => request::Event::KeysReleased { id },
                };
                dispatch_request(app, event);
            };
            own_copy(app, id, true);
            let (result, trace) = capture.capture_traced(&target, &progress);
            own_copy(app, id, false);
            let pdf = trace.from_chromium_pdf();
            (result, trace, pdf)
        }
        Trigger::DoubleCopy(DoubleCopy::NoSelection) => (
            Ok(CaptureOutcome::Unavailable(Unavailable::NoSelection)),
            CaptureTrace::default(),
            false,
        ),
        Trigger::DoubleCopy(DoubleCopy::Text(text)) => {
            let (context, trace) = capture.capture_context_traced(&target, &text);
            // The context is a bonus: without it the copied text is still looked up.
            let context = context.unwrap_or_else(|e| {
                eprintln!("capture: reading the context failed: {e}");
                None
            });
            let pdf = trace.from_chromium_pdf();
            let selection = Selection {
                text,
                context,
                url: None,
                method: CaptureMethod::ClipboardDoubleCopy,
            };
            (Ok(CaptureOutcome::Captured(selection)), trace, pdf)
        }
    }
}

#[cfg(windows)]
fn platform_capture() -> Arc<dyn TextCapture> {
    Arc::new(lextrail_capture::WindowsCapture::new())
}

#[cfg(not(windows))]
fn platform_capture() -> Arc<dyn TextCapture> {
    Arc::new(lextrail_capture::UnsupportedCapture)
}

/// Starts a lookup: records the foreground window and focus *before* the popup can
/// change anything, then captures in the background while the popup gets ready.
/// `pressed` is when the hotkey fired (or the double Ctrl+C was recognised).
///
/// The capture thread starts before the popup is asked to show: on the first lookup that
/// creates the window, which blocks this (main) thread for a while, and the capture runs
/// alongside it instead of after it (R3-P3).
pub fn lookup<R: Runtime>(
    app: &AppHandle<R>,
    pressed: Instant,
    trigger: Trigger,
) -> tauri::Result<()> {
    // Only the debug log uses it (`start +N ms`).
    #[cfg(not(debug_assertions))]
    let _ = pressed;
    let state = app.state::<PopupState>();
    let id = state.next_request.fetch_add(1, Ordering::SeqCst) + 1;
    if let Some(t) = &state.timings {
        t.start(id);
    }
    let target = state.capture.snapshot();
    // The popup itself is in front (hotkey pressed again while reading a result): keep
    // the result instead of trying to capture from the popup.
    if target
        .as_ref()
        .is_ok_and(|t| is_popup_window(app, t.native_window()))
    {
        #[cfg(debug_assertions)]
        eprintln!("hotkey ignored: the popup is in front");
        return Ok(());
    }
    // M5: the application blacklist is checked here, after the snapshot and before
    // the capture (ARCHITECTURE.md 4.1).

    let cursor = app.cursor_position()?;
    *lock(&state.cursor) = Some(Point {
        x: cursor.x.round() as i32,
        y: cursor.y.round() as i32,
    });
    state.target_window.store(
        target.as_ref().map_or(0, |t| t.native_window()),
        Ordering::SeqCst,
    );

    dispatch_request(app, request::Event::Started { id });

    let capture_app = app.clone();
    let capture = Arc::clone(&state.capture);
    spawn("popup-capture", move || {
        #[cfg(debug_assertions)]
        let start = pressed.elapsed();
        let source_app = target
            .as_ref()
            .ok()
            .and_then(|t| t.window.as_ref())
            .and_then(|w| w.process_name.clone());
        debug_capture_delay();
        let (result, trace, pdf_clipboard) =
            run_capture(&capture_app, id, capture.as_ref(), target, trigger);
        if let Err(e) = &result {
            eprintln!("capture failed: {e}");
        }
        let state = capture_app.state::<PopupState>();
        if let Some(t) = &state.timings {
            t.capture(id, trace.total);
        }
        let result = bench_lookup_text(result, state.timings.is_some());
        #[cfg(debug_assertions)]
        let label = result_label(&result);
        let lookup_start = Instant::now();
        let (content, record) = if matches!(result, Ok(CaptureOutcome::Captured(_))) {
            let dictionary = state.dictionary.get();
            Content::from_capture(result, pdf_clipboard, source_app, dictionary.as_ref())
        } else {
            Content::from_capture(result, false, source_app, None)
        };
        // One line per capture, without the captured text: method, result, time and
        // where the element came from; then the dictionary lookup's time.
        #[cfg(debug_assertions)]
        eprintln!(
            "capture #{id}: {label} | start +{:.1}ms; {trace}; lookup {:.1}ms",
            start.as_secs_f64() * 1000.0,
            lookup_start.elapsed().as_secs_f64() * 1000.0
        );
        #[cfg(not(debug_assertions))]
        let _ = lookup_start;
        dispatch_request(&capture_app, request::Event::Captured { id, content });
        // After the result is on its way: the first record opens the database.
        if let Some(record) = record {
            let started = Instant::now();
            let outcome = state.history.record(&record, |e| {
                crate::notice::warn(
                    &capture_app,
                    "LexTrail 查询历史",
                    format!(
                        "无法打开查询历史数据库（{e}），本次运行不记录查询历史，查词不受影响。"
                    ),
                );
            });
            // Without the text: recorded, repeat (deduplicated), or skipped (switched off or
            // the database is unavailable).
            #[cfg(debug_assertions)]
            eprintln!(
                "capture #{id}: history {} {:.1}ms",
                match outcome {
                    Some(lextrail_store::LookupOutcome::Recorded(_)) => "recorded",
                    Some(lextrail_store::LookupOutcome::Repeat) => "repeat",
                    Some(lextrail_store::LookupOutcome::Empty) => "empty",
                    None => "skipped",
                },
                started.elapsed().as_secs_f64() * 1000.0
            );
            #[cfg(not(debug_assertions))]
            let _ = (outcome, started);
        }
    });

    // May create the window and block for a while (first lookup).
    dispatch(app, Event::ShowRequested);
    Ok(())
}

/// Whether `native` is the popup window and it is on screen.
#[cfg(windows)]
fn is_popup_window<R: Runtime>(app: &AppHandle<R>, native: isize) -> bool {
    native != 0
        && app.get_webview_window(LABEL).is_some_and(|w| {
            w.is_visible().unwrap_or(false) && w.hwnd().is_ok_and(|h| h.0 as isize == native)
        })
}

#[cfg(not(windows))]
fn is_popup_window<R: Runtime>(_app: &AppHandle<R>, _native: isize) -> bool {
    false
}

/// Debug builds only: `LEXTRAIL_DEBUG_CAPTURE_DELAY_MS` slows every capture down, to
/// exercise the "capturing..." and foreground-change paths by hand.
fn debug_capture_delay() {
    #[cfg(debug_assertions)]
    if let Some(ms) = std::env::var("LEXTRAIL_DEBUG_CAPTURE_DELAY_MS")
        .ok()
        .and_then(|v| v.parse().ok())
    {
        thread::sleep(Duration::from_millis(ms));
    }
}

/// Benchmark only (`LEXTRAIL_BENCH_TIMINGS` set): `LEXTRAIL_BENCH_LOOKUP_TEXT` stands in
/// for a capture that found no text. The benchmark presses the hotkey over its own
/// terminal, where nothing is selected; with this it measures the app with the
/// dictionary open and a dictionary result on screen.
fn bench_lookup_text(
    result: Result<CaptureOutcome, CaptureError>,
    benchmark: bool,
) -> Result<CaptureOutcome, CaptureError> {
    if !benchmark || matches!(result, Ok(CaptureOutcome::Captured(_))) {
        return result;
    }
    match std::env::var("LEXTRAIL_BENCH_LOOKUP_TEXT") {
        Ok(text) if !text.is_empty() => Ok(CaptureOutcome::Captured(Selection {
            text,
            context: None,
            url: None,
            method: CaptureMethod::Uia,
        })),
        _ => result,
    }
}

/// Hides the popup (Esc, click outside).
pub fn hide<R: Runtime>(app: &AppHandle<R>) {
    dispatch(app, Event::HideRequested);
}

/// The system reported low memory: drop the popup unless it is on screen.
pub fn on_low_memory<R: Runtime>(app: &AppHandle<R>) {
    dispatch(app, Event::LowMemory);
}

/// Called by the popup page once it has subscribed to content events (or failed to,
/// in which case `error` says why). Only then may the window be revealed.
#[tauri::command]
pub fn popup_ready<R: Runtime>(app: AppHandle<R>, error: Option<String>) {
    if let Some(error) = error {
        eprintln!("popup page could not subscribe to content events: {error}");
    }
    dispatch(&app, Event::PageReady);
}

/// Called by the popup page when the user presses Esc.
#[tauri::command]
pub fn hide_popup<R: Runtime>(app: AppHandle<R>) {
    hide(&app);
}

fn dispatch<R: Runtime>(app: &AppHandle<R>, event: Event) {
    let state = app.state::<PopupState>();
    // Decide under the lock, act outside it: actions may call back into `dispatch`.
    let actions = lock(&state.lifecycle).handle(event, state.mode);
    #[cfg(debug_assertions)]
    eprintln!("popup: {event:?} -> {actions:?}");
    for action in actions {
        if let Err(e) = perform(app, action) {
            eprintln!("popup action {action:?} failed: {e}");
        }
    }
}

fn perform<R: Runtime>(app: &AppHandle<R>, action: Action) -> tauri::Result<()> {
    let window = app.get_webview_window(LABEL);
    match (action, window) {
        (Action::Create, _) => create(app),
        (Action::Reveal { resume }, Some(window)) => {
            if let Some(cursor) = *lock(&app.state::<PopupState>().cursor) {
                move_near(&window, cursor)?;
            }
            // The request decides when the window actually appears.
            let app = app.clone();
            let ready = move || dispatch_request(&app, request::Event::WindowReady);
            if resume {
                webview::resume_then(&window, ready)
            } else {
                ready();
                Ok(())
            }
        }
        (Action::Hide, Some(window)) => {
            hide_window(&window)?;
            dispatch_request(app, request::Event::Hidden);
            Ok(())
        }
        (Action::ScheduleSuspend { generation, after }, _) => {
            // One short-lived timer per hide; a newer hide makes this one stale.
            let app = app.clone();
            spawn("popup-suspend-timer", move || {
                thread::sleep(after);
                dispatch(&app, Event::SuspendTimerFired { generation });
            });
            Ok(())
        }
        (Action::Suspend, Some(window)) => {
            let app = app.clone();
            webview::suspend(&window, move |suspended| {
                dispatch(&app, Event::SuspendFinished { suspended });
            })
        }
        (Action::Destroy, Some(window)) => {
            window.destroy()?;
            dispatch_request(app, request::Event::Hidden);
            Ok(())
        }
        // The window is already gone; nothing to act on.
        (_, None) => Ok(()),
    }
}

fn dispatch_request<R: Runtime>(app: &AppHandle<R>, event: request::Event) {
    let state = app.state::<PopupState>();
    if let Some(t) = &state.timings {
        match &event {
            request::Event::WindowReady => t.window_ready(),
            request::Event::Activated { ok } => t.focused(*ok),
            _ => {}
        }
    }
    #[cfg(debug_assertions)]
    let label = format!("{event:?}");
    let actions = lock(&state.request).handle(event);
    #[cfg(debug_assertions)]
    eprintln!("request: {label} -> {actions:?}");
    for action in actions {
        if let Err(e) = perform_request(app, &action) {
            eprintln!("request action {action:?} failed: {e}");
        }
    }
}

fn perform_request<R: Runtime>(app: &AppHandle<R>, action: &request::Action) -> tauri::Result<()> {
    use request::Action as A;
    match action {
        A::SetContent(content) => app.emit_to(LABEL, content::EVENT, content),
        A::ShowInactive => match app.get_webview_window(LABEL) {
            Some(window) => {
                if let Some(t) = &app.state::<PopupState>().timings {
                    t.shown();
                }
                show_inactive(&window)
            }
            None => Ok(()),
        },
        A::Activate => match app.get_webview_window(LABEL) {
            Some(window) => activate(app, &window),
            None => Ok(()),
        },
        &A::WatchForeground(on) => app.run_on_main_thread(move || foreground::set_watching(on)),
        &A::StartTimer { id, timer, after } => {
            let app = app.clone();
            spawn("popup-timer", move || {
                thread::sleep(after);
                dispatch_request(&app, request::Event::TimerFired { id, timer });
            });
            Ok(())
        }
        A::Hide => {
            dispatch(app, Event::HideRequested);
            Ok(())
        }
    }
}

fn create<R: Runtime>(app: &AppHandle<R>) -> tauri::Result<()> {
    let window = WebviewWindowBuilder::new(app, LABEL, WebviewUrl::App("popup/index.html".into()))
        .title("LexTrail")
        .inner_size(WIDTH, HEIGHT)
        .decorations(false)
        .resizable(false)
        .maximizable(false)
        .minimizable(false)
        .always_on_top(true)
        .skip_taskbar(true)
        .shadow(true)
        .background_color(BACKGROUND)
        // Shown only after the page calls `popup_ready`, i.e. once it can receive
        // content events; a finished page load alone is not enough.
        .visible(false)
        .build()?;

    window.on_window_event({
        let window = window.clone();
        move |event| {
            // Clicking anywhere outside the focused popup moves focus away. WebView2 also
            // moves focus around while a hidden window initialises; ignore that.
            if let WindowEvent::Focused(false) = event
                && window.is_visible().unwrap_or(false)
            {
                hide(window.app_handle());
            }
        }
    });
    Ok(())
}

/// Shows the popup without taking focus from the window being read.
#[cfg(windows)]
fn show_inactive<R: Runtime>(window: &WebviewWindow<R>) -> tauri::Result<()> {
    use windows::Win32::UI::WindowsAndMessaging::{SW_SHOWNOACTIVATE, ShowWindowAsync};
    // Async: never blocks on the UI thread, whichever thread this runs on.
    // SAFETY: plain Win32 call on our own window handle.
    let _ = unsafe { ShowWindowAsync(window.hwnd()?, SW_SHOWNOACTIVATE) };
    Ok(())
}

#[cfg(not(windows))]
fn show_inactive<R: Runtime>(window: &WebviewWindow<R>) -> tauri::Result<()> {
    window.show()
}

/// Hides the window, whether it was shown by Tauri or by [`show_inactive`] (which Tauri
/// does not know about).
fn hide_window<R: Runtime>(window: &WebviewWindow<R>) -> tauri::Result<()> {
    window.hide()?;
    #[cfg(windows)]
    {
        use windows::Win32::UI::WindowsAndMessaging::{SW_HIDE, ShowWindowAsync};
        // SAFETY: plain Win32 call on our own window handle.
        let _ = unsafe { ShowWindowAsync(window.hwnd()?, SW_HIDE) };
    }
    Ok(())
}

/// Gives the popup focus (needed for Esc and click-outside), then reports whether it
/// worked (see [`activation`]). If Windows refuses, the popup stays visible without
/// focus: the request keeps watching the foreground, and the page's close button works.
fn activate<R: Runtime>(app: &AppHandle<R>, window: &WebviewWindow<R>) -> tauri::Result<()> {
    // Tell Tauri the window is visible, so its own hide calls keep working.
    window.show()?;
    let target = window.clone();
    app.run_on_main_thread(move || {
        // Debug builds only: LEXTRAIL_DEBUG_SKIP_ACTIVATION=1 leaves the popup without
        // focus, to test that state (close button, foreground watching) by hand.
        #[cfg(debug_assertions)]
        if std::env::var_os("LEXTRAIL_DEBUG_SKIP_ACTIVATION").is_some() {
            eprintln!("popup: activation skipped (LEXTRAIL_DEBUG_SKIP_ACTIVATION)");
            return;
        }
        let how = bring_to_front(&target);
        #[cfg(debug_assertions)]
        eprintln!("popup: bring to front: {how}");
        #[cfg(not(debug_assertions))]
        let _ = how;
    })?;
    let app = app.clone();
    let window = window.clone();
    spawn("popup-activation", move || {
        // Report as soon as the popup has focus; give up after ACTIVATION_CHECK.
        let deadline = std::time::Instant::now() + ACTIVATION_CHECK;
        let ok = loop {
            if is_foreground(&window) {
                break true;
            }
            if std::time::Instant::now() >= deadline {
                break false;
            }
            thread::sleep(Duration::from_millis(2));
        };
        #[cfg(debug_assertions)]
        eprintln!("popup: activated = {ok}");
        dispatch_request(&app, request::Event::Activated { ok });
    });
    Ok(())
}

#[cfg(windows)]
fn bring_to_front<R: Runtime>(window: &WebviewWindow<R>) -> String {
    match window.hwnd() {
        Ok(hwnd) => format!("{:?}", activation::bring_to_front(hwnd)),
        Err(e) => format!("no window handle: {e}"),
    }
}

#[cfg(not(windows))]
fn bring_to_front<R: Runtime>(window: &WebviewWindow<R>) -> String {
    match window.set_focus() {
        Ok(()) => "set_focus".into(),
        Err(e) => format!("set_focus failed: {e}"),
    }
}

#[cfg(windows)]
fn is_foreground<R: Runtime>(window: &WebviewWindow<R>) -> bool {
    use windows::Win32::UI::WindowsAndMessaging::GetForegroundWindow;
    // SAFETY: plain Win32 call.
    window
        .hwnd()
        .is_ok_and(|hwnd| unsafe { GetForegroundWindow() } == hwnd)
}

#[cfg(not(windows))]
fn is_foreground<R: Runtime>(window: &WebviewWindow<R>) -> bool {
    window.is_focused().unwrap_or(false)
}

fn move_near<R: Runtime>(window: &WebviewWindow<R>, cursor: Point) -> tauri::Result<()> {
    let Some(monitor) = window
        .monitor_from_point(cursor.x.into(), cursor.y.into())?
        .or(window.primary_monitor()?)
    else {
        return Ok(());
    };
    let scale = monitor.scale_factor();
    let area = monitor.work_area();
    let size = Size {
        width: (WIDTH * scale).round() as i32,
        height: (HEIGHT * scale).round() as i32,
    };
    let work_area = Rect {
        x: area.position.x,
        y: area.position.y,
        width: area.size.width as i32,
        height: area.size.height as i32,
    };
    let gap = (CURSOR_GAP * scale).round() as i32;
    let at = placement::place(cursor, size, work_area, gap);
    window.set_position(PhysicalPosition::new(at.x, at.y))?;
    // Also the size, for this monitor's current scale: moving between monitors, or a
    // scale changed while LexTrail runs, can leave the window sized for another scale
    // (R1-8). Set after the position, so it wins over any resize the move caused.
    window.set_size(PhysicalSize::new(size.width as u32, size.height as u32))
}

/// Starts a short-lived helper thread. Thread creation can fail (e.g. while the process
/// is being terminated); that is logged instead of panicking like `thread::spawn`.
fn spawn(name: &str, f: impl FnOnce() + Send + 'static) {
    if let Err(e) = thread::Builder::new().name(name.to_owned()).spawn(f) {
        eprintln!("cannot start thread {name}: {e}");
    }
}

fn lock<T>(mutex: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    // State stays consistent even if a panic happened while it was held.
    mutex.lock().unwrap_or_else(|e| e.into_inner())
}
