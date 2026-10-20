// Release builds are GUI-only: no console window behind the tray app.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod dictionary;
mod history;
mod hotkey;
mod keyboard;
mod main_window;
mod memory;
mod notice;
mod paths;
mod popup;
mod settings;
mod tray;

use tauri::RunEvent;

fn main() {
    tauri::Builder::default()
        // Must be registered first so a second launch exits before doing any work.
        .plugin(tauri_plugin_single_instance::init(|app, _args, _cwd| {
            main_window::open(app);
        }))
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_global_shortcut::Builder::new().build())
        .invoke_handler(tauri::generate_handler![popup::hide_popup, popup::popup_ready])
        .setup(|app| {
            let handle = app.handle();
            tray::create(handle)?;
            let settings = settings::load(handle);
            popup::init(handle, settings.popup.mode, settings.history.record);
            memory::watch({
                let handle = handle.clone();
                move || popup::on_low_memory(&handle)
            });
            let failures = hotkey::register_all(handle, &hotkey::bindings_from(&settings.hotkeys));
            hotkey::notify_failures(handle, &failures);
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
