//! The lookup popup: a single reusable window (D010), created on first use so the
//! tray-only state never loads a WebView.

mod placement;

use tauri::webview::PageLoadEvent;
use tauri::window::Color;
use tauri::{
    AppHandle, Manager, PhysicalPosition, Runtime, WebviewUrl, WebviewWindow, WebviewWindowBuilder,
    WindowEvent,
};

use placement::{Point, Rect, Size};

pub const LABEL: &str = "popup";

/// Logical size; real dimensions follow once the popup has content.
const WIDTH: f64 = 380.0;
const HEIGHT: f64 = 260.0;
/// Logical distance between the cursor and the popup edge.
const CURSOR_GAP: f64 = 12.0;
/// Matches the page background so the window never flashes white.
const BACKGROUND: Color = Color(255, 255, 255, 255);

/// Shows the popup next to the mouse cursor, creating it on first use.
pub fn show_at_cursor<R: Runtime>(app: &AppHandle<R>) -> tauri::Result<()> {
    let cursor = app.cursor_position()?;
    let cursor = Point {
        x: cursor.x.round() as i32,
        y: cursor.y.round() as i32,
    };
    match app.get_webview_window(LABEL) {
        Some(window) => {
            move_near(&window, cursor)?;
            reveal(&window)
        }
        None => create(app, cursor),
    }
}

/// Hides the popup (Esc, click outside). The window is kept for reuse.
pub fn hide<R: Runtime>(app: &AppHandle<R>) -> tauri::Result<()> {
    match app.get_webview_window(LABEL) {
        Some(window) => window.hide(),
        None => Ok(()),
    }
}

/// Called by the popup page when the user presses Esc.
#[tauri::command]
pub fn hide_popup<R: Runtime>(app: AppHandle<R>) -> Result<(), String> {
    hide(&app).map_err(|e| e.to_string())
}

fn create<R: Runtime>(app: &AppHandle<R>, cursor: Point) -> tauri::Result<()> {
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
            if payload.event() == PageLoadEvent::Finished
                && let Err(e) = reveal(&window)
            {
                eprintln!("cannot show popup: {e}");
            }
        })
        .build()?;

    window.on_window_event({
        let window = window.clone();
        move |event| {
            // Clicking anywhere outside the popup moves focus away.
            if let WindowEvent::Focused(false) = event {
                let _ = window.hide();
            }
        }
    });
    move_near(&window, cursor)
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
