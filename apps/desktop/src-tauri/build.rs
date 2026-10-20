/// The app's own commands. Listing them makes each one need a permission
/// (`allow-<command>`), so a window can only call the commands its capability grants
/// (`docs/specs/m1-hotkey-settings.md` 1.3).
const COMMANDS: &[&str] = &[
    // Popup.
    "hide_popup",
    "popup_ready",
    "popup_measured",
    "popup_resize",
    // Settings window.
    "settings_get",
    "hotkey_recording",
    "hotkey_check",
    "hotkey_apply",
    "settings_set_double_ctrl_c",
    "settings_set_record_history",
];

fn main() {
    tauri_build::try_build(
        tauri_build::Attributes::new()
            .app_manifest(tauri_build::AppManifest::new().commands(COMMANDS)),
    )
    .expect("failed to run tauri-build");
}
