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
mod settings_window;
mod tray;

use tauri::{Manager, RunEvent};

/// Opens the settings window instead of the main window, at first launch or when the
/// app is already running (`docs/specs/m1-hotkey-settings.md` 1.1).
const SETTINGS_ARG: &str = "--settings";

fn wants_settings<S: AsRef<str>>(args: &[S]) -> bool {
    args.iter().skip(1).any(|a| a.as_ref() == SETTINGS_ARG)
}

fn main() {
    tauri::Builder::default()
        // Must be registered first so a second launch exits before doing any work.
        .plugin(tauri_plugin_single_instance::init(|app, args, _cwd| {
            if wants_settings(&args) {
                settings_window::open(app);
            } else {
                main_window::open(app);
            }
        }))
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_global_shortcut::Builder::new().build())
        .invoke_handler(tauri::generate_handler![
            popup::hide_popup,
            popup::popup_ready,
            popup::popup_measured,
            popup::popup_resize,
            settings_window::settings_get,
            settings_window::hotkey_recording,
            settings_window::hotkey_check,
            settings_window::hotkey_apply,
            settings_window::settings_set_double_ctrl_c,
            settings_window::settings_set_record_history,
            settings_window::settings_pick_dict_folder,
            settings_window::settings_reset_dict_folder,
            settings_window::settings_set_oxford,
            settings_window::settings_set_english_definitions
        ])
        .setup(|app| {
            let handle = app.handle();
            tray::create(handle)?;
            let service = settings::SettingsService::load(handle);
            let settings = service.settings();
            app.manage(service);
            let benchmark = std::env::var_os(popup::timing::ENV).is_some();
            app.manage(dictionary::ExtDictionaryService::new(
                paths::default_dicts_dir(handle, benchmark),
                &settings.dictionaries,
            ));
            popup::init(
                handle,
                settings.popup.mode,
                settings.history.record,
                settings.hotkeys.double_ctrl_c,
            );
            memory::watch({
                let handle = handle.clone();
                move || popup::on_low_memory(&handle)
            });
            hotkey::init(handle, &settings.hotkeys.lookup);
            let args: Vec<String> = std::env::args().collect();
            if wants_settings(&args) {
                settings_window::open(handle);
            }
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn settings_argument() {
        assert!(wants_settings(&["lextrail-desktop.exe", "--settings"]));
        assert!(!wants_settings(&["lextrail-desktop.exe"]));
        // The program path itself never counts.
        assert!(!wants_settings(&["--settings"]));
        assert!(!wants_settings(&["lextrail-desktop.exe", "--settings=1"]));
    }

    /// Commands in `generate_handler!` by module: `popup::x` belongs to the popup window,
    /// `settings_window::x` to the settings window.
    fn handlers(module: &str) -> Vec<String> {
        let source = include_str!("main.rs");
        let start = source.find("generate_handler![").unwrap();
        let end = start + source[start..].find(']').unwrap();
        let mut names: Vec<String> = source[start..end]
            .split([',', '[', '\n'])
            .filter_map(|item| item.trim().strip_prefix(module)?.strip_prefix("::"))
            .map(str::to_owned)
            .collect();
        names.sort();
        names
    }

    /// The `allow-<command>` permissions a capability file grants, as command names.
    fn granted(capability: &str) -> Vec<String> {
        let json: serde_json::Value = serde_json::from_str(capability).unwrap();
        let mut names: Vec<String> = json["permissions"]
            .as_array()
            .unwrap()
            .iter()
            .filter_map(|p| p.as_str()?.strip_prefix("allow-"))
            .map(|p| p.replace('-', "_"))
            .collect();
        names.sort();
        names
    }

    #[test]
    fn each_window_may_call_only_its_own_commands() {
        let popup = handlers("popup");
        let settings = handlers("settings_window");
        assert_eq!(popup.len(), 4);
        assert_eq!(settings.len(), 10);
        assert_eq!(granted(include_str!("../capabilities/popup.json")), popup);
        assert_eq!(
            granted(include_str!("../capabilities/settings.json")),
            settings
        );
        // build.rs lists every command, so none is callable without a permission.
        let manifest = include_str!("../build.rs");
        for command in popup.iter().chain(&settings) {
            assert!(manifest.contains(&format!("\"{command}\"")), "{command}");
        }
    }
}
