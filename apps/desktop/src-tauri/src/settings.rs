//! The settings: loaded at startup, changed from the settings window and written back
//! right away (`docs/specs/m1-hotkey-settings.md` 5.3).

use std::sync::Mutex;

use lextrail_config::{
    ConfigError, FileStamp, LoadOutcome, SCHEMA_VERSION, Settings, SettingsFile,
};
use tauri::{AppHandle, Runtime};

const TITLE: &str = "LexTrail 设置文件";
const CHANGED_ON_DISK: &str = "设置文件被手动修改过，请重启 LexTrail 后再修改";

/// The settings in memory plus where they are saved. Every change edits one field of
/// the in-memory copy and writes the whole file, so other fields are never lost.
pub struct SettingsService {
    inner: Mutex<Inner>,
}

struct Inner {
    settings: Settings,
    /// `Err`: changes cannot be saved, and why (shown in the settings window).
    target: Result<(SettingsFile, FileStamp), String>,
}

impl SettingsService {
    /// Loads the settings from the app config directory. Problems never stop the app:
    /// the user is told and defaults are used (and the window is read-only).
    pub fn load<R: Runtime>(app: &AppHandle<R>) -> Self {
        let (settings, target) = match crate::paths::config_dir(app) {
            Ok(dir) => load_file(app, SettingsFile::in_dir(dir)),
            Err(e) => {
                eprintln!("cannot resolve config directory: {e}");
                crate::notice::warn(
                    app,
                    TITLE,
                    format!("无法确定设置文件的位置（{e}），本次使用默认设置。"),
                );
                (
                    Settings::default(),
                    Err("无法确定设置文件的位置，本次不能修改设置".to_owned()),
                )
            }
        };
        Self::new(settings, target)
    }

    fn new(settings: Settings, target: Result<(SettingsFile, FileStamp), String>) -> Self {
        Self {
            inner: Mutex::new(Inner { settings, target }),
        }
    }

    pub fn settings(&self) -> Settings {
        self.inner.lock().unwrap().settings.clone()
    }

    /// Why the settings cannot be changed from the window, if they cannot.
    pub fn read_only(&self) -> Option<String> {
        self.inner.lock().unwrap().target.as_ref().err().cloned()
    }

    /// Applies `change` and saves. On failure nothing changes, in memory or on disk, and
    /// the error is for the user.
    pub fn update(&self, change: impl FnOnce(&mut Settings)) -> Result<(), String> {
        let mut inner = self.inner.lock().unwrap();
        let mut settings = inner.settings.clone();
        change(&mut settings);
        let (file, stamp) = inner.target.as_mut().map_err(|why| why.clone())?;
        match file.save_if_unchanged(&settings, *stamp) {
            Ok(new_stamp) => {
                *stamp = new_stamp;
                inner.settings = settings;
                Ok(())
            }
            Err(ConfigError::ChangedOnDisk { .. }) => Err(CHANGED_ON_DISK.to_owned()),
            Err(e) => {
                eprintln!("cannot save settings: {e}");
                Err(format!("无法保存设置文件：{e}"))
            }
        }
    }
}

fn load_file<R: Runtime>(
    app: &AppHandle<R>,
    file: SettingsFile,
) -> (Settings, Result<(SettingsFile, FileStamp), String>) {
    let outcome = match file.load() {
        Ok(outcome) => outcome,
        Err(e) => {
            eprintln!("cannot load settings: {e}");
            crate::notice::warn(
                app,
                TITLE,
                format!("无法读取设置文件（{e}），本次使用默认设置。"),
            );
            return (
                Settings::default(),
                Err("无法读取设置文件，本次不能修改设置".to_owned()),
            );
        }
    };
    if let Some(message) = outcome_message(&outcome) {
        crate::notice::warn(app, TITLE, message);
    }
    let settings = outcome.into_settings();
    let target = target_for(&settings, file);
    (settings, target)
}

fn target_for(
    settings: &Settings,
    file: SettingsFile,
) -> Result<(SettingsFile, FileStamp), String> {
    if settings.version > SCHEMA_VERSION {
        return Err("设置文件来自更新版本的 LexTrail，不能在这里修改".to_owned());
    }
    let stamp = file.stamp().map_err(|e| {
        eprintln!("cannot read the settings file's stamp: {e}");
        "无法读取设置文件，本次不能修改设置".to_owned()
    })?;
    Ok((file, stamp))
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
    use std::fs;
    use std::path::PathBuf;

    use super::*;

    fn service_in(dir: &std::path::Path) -> SettingsService {
        let file = SettingsFile::in_dir(dir);
        let settings = file.load().unwrap().into_settings();
        let target = target_for(&settings, file);
        SettingsService::new(settings, target)
    }

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

    #[test]
    fn updates_are_saved_one_field_at_a_time() {
        let dir = tempfile::tempdir().unwrap();
        let service = service_in(dir.path());
        service.update(|s| s.history.record = false).unwrap();
        service.update(|s| s.hotkeys.lookup = "F8".into()).unwrap();
        let on_disk = SettingsFile::in_dir(dir.path())
            .load()
            .unwrap()
            .into_settings();
        assert!(!on_disk.history.record);
        assert_eq!(on_disk.hotkeys.lookup, "F8");
        assert_eq!(service.settings(), on_disk);
        assert_eq!(service.read_only(), None);
    }

    #[test]
    fn a_hand_edit_is_not_overwritten() {
        let dir = tempfile::tempdir().unwrap();
        let service = service_in(dir.path());
        let path = SettingsFile::in_dir(dir.path()).path().to_owned();
        let edited = "version = 1\n[hotkeys]\nlookup = \"F9\"\n# mine\n";
        fs::write(&path, edited).unwrap();
        let err = service.update(|s| s.history.record = false).unwrap_err();
        assert_eq!(err, CHANGED_ON_DISK);
        assert_eq!(fs::read_to_string(&path).unwrap(), edited);
        // Memory is unchanged too, and later changes are refused the same way.
        assert!(service.settings().history.record);
        assert_eq!(
            service.update(|s| s.hotkeys.double_ctrl_c = true),
            Err(CHANGED_ON_DISK.to_owned())
        );
    }

    #[test]
    fn a_newer_file_is_read_only() {
        let dir = tempfile::tempdir().unwrap();
        let file = SettingsFile::in_dir(dir.path());
        fs::write(file.path(), format!("version = {}\n", SCHEMA_VERSION + 1)).unwrap();
        let service = service_in(dir.path());
        assert!(service.read_only().unwrap().contains("更新版本"));
        assert!(service.update(|s| s.history.record = false).is_err());
    }
}
