//! User-facing notices that work without any WebView window.

use tauri::{AppHandle, Runtime};
use tauri_plugin_dialog::{DialogExt, MessageDialogKind};

/// Shows a non-blocking native warning message box.
pub fn warn<R: Runtime>(app: &AppHandle<R>, title: &str, message: String) {
    app.dialog()
        .message(message)
        .title(title)
        .kind(MessageDialogKind::Warning)
        .show(|_| {});
}
