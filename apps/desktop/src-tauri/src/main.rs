// Release builds are GUI-only: no console window behind the tray app.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod tray;

use tauri::RunEvent;

fn main() {
    tauri::Builder::default()
        // Must be registered first so a second launch exits before doing any work.
        .plugin(tauri_plugin_single_instance::init(|_app, _args, _cwd| {
            // TODO(M0 main window): bring the main window to the front.
        }))
        .setup(|app| {
            tray::create(app.handle())?;
            Ok(())
        })
        .build(tauri::generate_context!())
        .expect("failed to build Tauri application")
        .run(|_app, event| {
            // Windows are destroyed on close; the app lives on in the tray until "Quit"
            // (which exits with an explicit code).
            if let RunEvent::ExitRequested { code: None, api, .. } = event {
                api.prevent_exit();
            }
        });
}
