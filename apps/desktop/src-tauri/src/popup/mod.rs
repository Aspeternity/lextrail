//! The lookup popup: a single reusable window (D010), created on first use so the
//! tray-only state never loads a WebView. Its lifecycle (D009) lives in [`lifecycle`];
//! this module turns the lifecycle's actions into window and WebView2 calls.

mod lifecycle;
mod placement;
mod webview;

use std::sync::Mutex;
use std::thread;

use lextrail_config::PopupMode;
use tauri::webview::PageLoadEvent;
use tauri::window::Color;
use tauri::{
    AppHandle, Manager, PhysicalPosition, Runtime, WebviewUrl, WebviewWindow, WebviewWindowBuilder,
    WindowEvent,
};

use lifecycle::{Action, Event, Lifecycle};
use placement::{Point, Rect, Size};

pub const LABEL: &str = "popup";

/// Logical size; real dimensions follow once the popup has content.
const WIDTH: f64 = 380.0;
const HEIGHT: f64 = 260.0;
/// Logical distance between the cursor and the popup edge.
const CURSOR_GAP: f64 = 12.0;
/// Matches the page background so the window never flashes white.
const BACKGROUND: Color = Color(255, 255, 255, 255);

/// Shared popup state, managed by Tauri.
struct PopupState {
    lifecycle: Mutex<Lifecycle>,
    mode: PopupMode,
    /// Cursor position of the latest show request; used when the popup is revealed.
    cursor: Mutex<Option<Point>>,
}

/// Registers the popup state. Call once during setup.
pub fn init<R: Runtime>(app: &AppHandle<R>, mode: PopupMode) {
    app.manage(PopupState {
        lifecycle: Mutex::new(Lifecycle::new()),
        mode,
        cursor: Mutex::new(None),
    });
}

/// Shows the popup next to the mouse cursor (creating or resuming it as needed).
pub fn show_at_cursor<R: Runtime>(app: &AppHandle<R>) -> tauri::Result<()> {
    let cursor = app.cursor_position()?;
    *lock(&app.state::<PopupState>().cursor) = Some(Point {
        x: cursor.x.round() as i32,
        y: cursor.y.round() as i32,
    });
    dispatch(app, Event::ShowRequested);
    Ok(())
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
            if resume {
                webview::resume_then(&window, reveal)
            } else {
                reveal(&window)
            }
        }
        (Action::Hide, Some(window)) => window.hide(),
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
        (Action::Destroy, Some(window)) => window.destroy(),
        // The window is already gone; nothing to act on.
        (_, None) => Ok(()),
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
            // Clicking anywhere outside the popup moves focus away. WebView2 also moves
            // focus around while a hidden window initialises; ignore that.
            if let WindowEvent::Focused(false) = event
                && window.is_visible().unwrap_or(false)
            {
                hide(window.app_handle());
            }
        }
    });
    Ok(())
}

fn reveal<R: Runtime>(window: &WebviewWindow<R>) -> tauri::Result<()> {
    window.show()?;
    // Focus is needed for Esc and for "click outside" (focus lost) to work. Windows
    // allows it because the hotkey press was input to this process.
    window.set_focus()
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
