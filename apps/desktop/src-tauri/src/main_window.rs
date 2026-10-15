//! The main window (word book, review, settings in later milestones). It is created on
//! demand and destroyed when closed, so the tray-only state never keeps its WebView
//! alive. It shares the app's single WebView2 environment with the popup (D010).

use tauri::webview::PageLoadEvent;
use tauri::window::Color;
use tauri::{AppHandle, Manager, Runtime, WebviewUrl, WebviewWindowBuilder};

pub const LABEL: &str = "main";

/// Logical size of a newly created window.
const WIDTH: f64 = 960.0;
const HEIGHT: f64 = 640.0;
const MIN_WIDTH: f64 = 640.0;
const MIN_HEIGHT: f64 = 420.0;
/// Matches the page background so the window never flashes white.
const BACKGROUND: Color = Color(255, 255, 255, 255);

/// Brings the main window to the front, creating it if it does not exist.
pub fn open<R: Runtime>(app: &AppHandle<R>) {
    if let Err(e) = try_open(app) {
        eprintln!("failed to open main window: {e}");
    }
}

fn try_open<R: Runtime>(app: &AppHandle<R>) -> tauri::Result<()> {
    if let Some(window) = app.get_webview_window(LABEL) {
        // Still loading: the page-load handler shows it.
        if !window.is_visible()? {
            return Ok(());
        }
        if window.is_minimized()? {
            window.unminimize()?;
        }
        window.show()?;
        return window.set_focus();
    }

    // Closing destroys the window (Tauri's default); the app stays in the tray.
    WebviewWindowBuilder::new(app, LABEL, WebviewUrl::App("main/index.html".into()))
        .title("LexTrail")
        .inner_size(WIDTH, HEIGHT)
        .min_inner_size(MIN_WIDTH, MIN_HEIGHT)
        .center()
        .background_color(BACKGROUND)
        .visible(false)
        // Show only once the page has rendered, so the first appearance is complete.
        .on_page_load(|window, payload| {
            if payload.event() == PageLoadEvent::Finished
                && !window.is_visible().unwrap_or(true)
            {
                let shown = window.show().and_then(|()| window.set_focus());
                if let Err(e) = shown {
                    eprintln!("failed to show main window: {e}");
                }
            }
        })
        .build()?;
    Ok(())
}
