//! Loading the settings file at startup.

use lextrail_config::{LoadOutcome, Settings, SettingsFile};
use tauri::{AppHandle, Runtime};

const TITLE: &str = "LexTrail 设置文件";

/// Loads the settings from the app config directory. Problems never stop the app:
/// the user is told and defaults are used.
pub fn load<R: Runtime>(app: &AppHandle<R>) -> Settings {
    let dir = match crate::paths::config_dir(app) {
        Ok(dir) => dir,
        Err(e) => {
            eprintln!("cannot resolve config directory: {e}");
            crate::notice::warn(
                app,
                TITLE,
                format!("无法确定设置文件的位置（{e}），本次使用默认设置。"),
            );
            return Settings::default();
        }
    };
    let file = SettingsFile::in_dir(dir);
    match file.load() {
        Ok(outcome) => {
            if let Some(message) = outcome_message(&outcome) {
                crate::notice::warn(app, TITLE, message);
            }
            outcome.into_settings()
        }
        Err(e) => {
            eprintln!("cannot load settings: {e}");
            crate::notice::warn(
                app,
                TITLE,
                format!("无法读取设置文件（{e}），本次使用默认设置。"),
            );
            Settings::default()
        }
    }
}

/// What the user should be told about a load, if anything.
fn outcome_message(outcome: &LoadOutcome) -> Option<String> {
    match outcome {
        LoadOutcome::Loaded(_) | LoadOutcome::Created(_) => None,
        LoadOutcome::Recovered { backup, .. } => Some(format!(
            "设置文件格式有误，已恢复为默认设置。\n原文件已备份到：\n{}",
            backup.display()
        )),
    }
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use super::*;

    #[test]
    fn normal_loads_are_silent() {
        assert_eq!(
            outcome_message(&LoadOutcome::Loaded(Settings::default())),
            None
        );
        assert_eq!(
            outcome_message(&LoadOutcome::Created(Settings::default())),
            None
        );
    }

    #[test]
    fn recovery_message_names_the_backup() {
        let outcome = LoadOutcome::Recovered {
            settings: Settings::default(),
            backup: PathBuf::from(r"C:\cfg\settings.toml.corrupt"),
        };
        let message = outcome_message(&outcome).unwrap();
        assert!(message.contains("默认设置"));
        assert!(message.contains(r"C:\cfg\settings.toml.corrupt"));
    }
}
