//! The lookup popup: a single reusable window (D010), created on first use so the
//! tray-only state never loads a WebView.
//!
//! - [`lifecycle`]: whether the window exists and is awake (D009).
//! - [`request`]: one lookup, from hotkey to focused result; when the window appears and
//!   when it takes focus (ARCHITECTURE.md 9.3).
//!
//! This module turns both state machines' actions into window, WebView2 and capture calls.

mod content;
mod foreground;
mod lifecycle;
mod placement;
mod request;
mod webview;

use std::sync::atomic::{AtomicIsize, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Duration;

use lextrail_capture::TextCapture;
use lextrail_config::PopupMode;
use tauri::webview::PageLoadEvent;
use tauri::window::Color;
use tauri::{
    AppHandle, Emitter, Manager, PhysicalPosition, Runtime, WebviewUrl, WebviewWindow,
    WebviewWindowBuilder, WindowEvent,
};

use content::Content;
use lifecycle::{Action, Event, Lifecycle};
use placement::{Point, Rect, Size};
use request::Request;

pub const LABEL: &str = "popup";

/// Logical size; real dimensions follow once the popup has content.
const WIDTH: f64 = 380.0;
const HEIGHT: f64 = 260.0;
/// Logical distance between the cursor and the popup edge.
const CURSOR_GAP: f64 = 12.0;
/// Matches the page background so the window never flashes white.
const BACKGROUND: Color = Color(255, 255, 255, 255);
/// How long after asking for focus the result is checked.
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
}

/// Registers the popup state. Call once during setup.
pub fn init<R: Runtime>(app: &AppHandle<R>, mode: PopupMode) {
    app.manage(PopupState {
        lifecycle: Mutex::new(Lifecycle::new()),
        request: Mutex::new(Request::new()),
        mode,
        cursor: Mutex::new(None),
        capture: platform_capture(),
        next_request: AtomicU64::new(0),
        target_window: AtomicIsize::new(0),
    });
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
pub fn lookup<R: Runtime>(app: &AppHandle<R>) -> tauri::Result<()> {
    let state = app.state::<PopupState>();
    let target = state.capture.snapshot();
    // M5: the application blacklist is checked here, after the snapshot and before
    // the capture (ARCHITECTURE.md 4.1).

    let cursor = app.cursor_position()?;
    *lock(&state.cursor) = Some(Point {
        x: cursor.x.round() as i32,
        y: cursor.y.round() as i32,
    });
    let id = state.next_request.fetch_add(1, Ordering::SeqCst) + 1;
    state.target_window.store(
        target.as_ref().map_or(0, |t| t.native_window()),
        Ordering::SeqCst,
    );

    dispatch_request(app, request::Event::Started { id });
    dispatch(app, Event::ShowRequested);

    let timer_app = app.clone();
    thread::spawn(move || {
        thread::sleep(request::LOADING_DELAY);
        dispatch_request(&timer_app, request::Event::DelayElapsed { id });
    });

    let capture_app = app.clone();
    let capture = Arc::clone(&state.capture);
    thread::spawn(move || {
        let source_app = target
            .as_ref()
            .ok()
            .and_then(|t| t.window.as_ref())
            .and_then(|w| w.process_name.clone());
        debug_capture_delay();
        let result = target.and_then(|t| capture.capture(&t));
        if let Err(e) = &result {
            eprintln!("capture failed: {e}");
        }
        #[cfg(debug_assertions)]
        eprintln!("capture #{id}: {result:?}");
        let content = Content::from_capture(result, source_app);
        dispatch_request(&capture_app, request::Event::Captured { id, content });
    });
    Ok(())
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

/// Hides the popup (Esc, click outside).
pub fn hide<R: Runtime>(app: &AppHandle<R>) {
    dispatch(app, Event::HideRequested);
}

/// The system reported low memory: drop the popup unless it is on screen.
pub fn on_low_memory<R: Runtime>(app: &AppHandle<R>) {
    dispatch(app, Event::LowMemory);
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
            thread::spawn(move || {
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
            Some(window) => show_inactive(&window),
            None => Ok(()),
        },
        A::Activate => match app.get_webview_window(LABEL) {
            Some(window) => activate(app, &window),
            None => Ok(()),
        },
        &A::WatchForeground(on) => app.run_on_main_thread(move || foreground::set_watching(on)),
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
        .visible(false)
        // Show only once the page has rendered, so the first appearance is complete.
        .on_page_load(|window, payload| {
            if payload.event() == PageLoadEvent::Finished {
                dispatch(window.app_handle(), Event::PageLoaded);
            }
        })
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
/// worked. Windows allows it because the hotkey press was input to this process; it can
/// still refuse (foreground lock), and the request then keeps watching the foreground.
fn activate<R: Runtime>(app: &AppHandle<R>, window: &WebviewWindow<R>) -> tauri::Result<()> {
    // Tell Tauri the window is visible, so its own focus and hide calls work.
    window.show()?;
    let target = window.clone();
    app.run_on_main_thread(move || {
        if !try_set_foreground(&target) {
            #[cfg(debug_assertions)]
            eprintln!("popup: SetForegroundWindow refused; falling back to set_focus");
            if let Err(e) = target.set_focus() {
                eprintln!("popup: set_focus failed: {e}");
            }
        }
    })?;
    let app = app.clone();
    let window = window.clone();
    thread::spawn(move || {
        thread::sleep(ACTIVATION_CHECK);
        let ok = is_foreground(&window);
        #[cfg(debug_assertions)]
        eprintln!("popup: activated = {ok}");
        dispatch_request(&app, request::Event::Activated { ok });
    });
    Ok(())
}

#[cfg(windows)]
fn try_set_foreground<R: Runtime>(window: &WebviewWindow<R>) -> bool {
    use windows::Win32::UI::WindowsAndMessaging::SetForegroundWindow;
    // SAFETY: plain Win32 call on our own window handle.
    window
        .hwnd()
        .is_ok_and(|hwnd| unsafe { SetForegroundWindow(hwnd) }.as_bool())
}

#[cfg(not(windows))]
fn try_set_foreground<R: Runtime>(_window: &WebviewWindow<R>) -> bool {
    false
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
    window.set_position(PhysicalPosition::new(at.x, at.y))
}

fn lock<T>(mutex: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    // State stays consistent even if a panic happened while it was held.
    mutex.lock().unwrap_or_else(|e| e.into_inner())
}
