//! Reading and writing the settings file.

use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use crate::settings::{SCHEMA_VERSION, Settings};

const FILE_NAME: &str = "settings.toml";
const CORRUPT_SUFFIX: &str = "corrupt";
const TMP_SUFFIX: &str = "tmp";

const HEADER: &str = "\
# LexTrail settings. Restart LexTrail after editing this file by hand.
# API keys and sync tokens are NOT stored here; they live in the system credential manager.

";

#[derive(Debug, thiserror::Error)]
pub enum ConfigError {
    #[error("failed to access settings file {path}: {source}")]
    Io {
        path: PathBuf,
        #[source]
        source: io::Error,
    },
    #[error("failed to serialize settings: {0}")]
    Serialize(#[from] toml::ser::Error),
    #[error(
        "settings file was written by a newer LexTrail (schema {found}, this build supports \
         {SCHEMA_VERSION}); refusing to overwrite it"
    )]
    NewerVersion { found: u32 },
}

/// What [`SettingsFile::load`] found on disk. Every variant carries usable settings.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LoadOutcome {
    /// The file existed and was read.
    Loaded(Settings),
    /// There was no file; defaults were written to disk.
    Created(Settings),
    /// The file could not be parsed. It was moved to `backup` and defaults were written.
    Recovered { settings: Settings, backup: PathBuf },
}

impl LoadOutcome {
    pub fn settings(&self) -> &Settings {
        match self {
            Self::Loaded(s) | Self::Created(s) | Self::Recovered { settings: s, .. } => s,
        }
    }

    pub fn into_settings(self) -> Settings {
        match self {
            Self::Loaded(s) | Self::Created(s) | Self::Recovered { settings: s, .. } => s,
        }
    }
}

/// The settings file inside the app's config directory.
#[derive(Debug, Clone)]
pub struct SettingsFile {
    path: PathBuf,
}

impl SettingsFile {
    pub fn in_dir(dir: impl AsRef<Path>) -> Self {
        Self {
            path: dir.as_ref().join(FILE_NAME),
        }
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    /// Loads the settings, creating the file with defaults if it is missing and
    /// recovering (backup + defaults) if it cannot be parsed. Only I/O failures are errors.
    pub fn load(&self) -> Result<LoadOutcome, ConfigError> {
        let text = match fs::read_to_string(&self.path) {
            Ok(text) => text,
            Err(e) if e.kind() == io::ErrorKind::NotFound => {
                let settings = Settings::default();
                self.save(&settings)?;
                return Ok(LoadOutcome::Created(settings));
            }
            Err(e) => return Err(self.io_error(e)),
        };
        match toml::from_str::<Settings>(&text) {
            Ok(settings) => Ok(LoadOutcome::Loaded(settings)),
            Err(_) => {
                let backup = self.sibling(CORRUPT_SUFFIX);
                fs::rename(&self.path, &backup).map_err(|e| self.io_error(e))?;
                let settings = Settings::default();
                self.save(&settings)?;
                Ok(LoadOutcome::Recovered { settings, backup })
            }
        }
    }

    /// Writes the settings atomically (temp file + rename), creating the directory if needed.
    pub fn save(&self, settings: &Settings) -> Result<(), ConfigError> {
        if settings.version > SCHEMA_VERSION {
            return Err(ConfigError::NewerVersion {
                found: settings.version,
            });
        }
        let text = format!("{HEADER}{}", toml::to_string(settings)?);
        if let Some(dir) = self.path.parent() {
            fs::create_dir_all(dir).map_err(|e| self.io_error(e))?;
        }
        let tmp = self.sibling(TMP_SUFFIX);
        fs::write(&tmp, text).map_err(|e| self.io_error(e))?;
        // `rename` replaces an existing file on Windows too (MOVEFILE_REPLACE_EXISTING).
        fs::rename(&tmp, &self.path).map_err(|e| self.io_error(e))
    }

    fn sibling(&self, suffix: &str) -> PathBuf {
        let mut name = self.path.file_name().unwrap_or_default().to_os_string();
        name.push(".");
        name.push(suffix);
        self.path.with_file_name(name)
    }

    fn io_error(&self, source: io::Error) -> ConfigError {
        ConfigError::Io {
            path: self.path.clone(),
            source,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::settings::PopupMode;

    fn temp_file() -> (tempfile::TempDir, SettingsFile) {
        let dir = tempfile::tempdir().unwrap();
        // A nested directory checks that missing parents are created.
        let file = SettingsFile::in_dir(dir.path().join("app"));
        (dir, file)
    }

    #[test]
    fn missing_file_is_created_with_defaults() {
        let (_dir, file) = temp_file();
        let outcome = file.load().unwrap();
        assert_eq!(outcome, LoadOutcome::Created(Settings::default()));
        assert!(file.path().exists());
        assert_eq!(
            file.load().unwrap(),
            LoadOutcome::Loaded(Settings::default())
        );
    }

    #[test]
    fn save_then_load_round_trips() {
        let (_dir, file) = temp_file();
        let mut settings = Settings::default();
        settings.popup.mode = PopupMode::MemorySaver;
        settings.hotkeys.lookup = "Ctrl+Alt+F9".into();
        file.save(&settings).unwrap();
        assert_eq!(file.load().unwrap().into_settings(), settings);
    }

    #[test]
    fn saved_file_has_header_and_no_temp_file_left() {
        let (_dir, file) = temp_file();
        file.save(&Settings::default()).unwrap();
        let text = fs::read_to_string(file.path()).unwrap();
        assert!(text.starts_with("# LexTrail settings."));
        assert!(text.contains("credential manager"));
        assert!(!file.sibling(TMP_SUFFIX).exists());
    }

    #[test]
    fn hand_edited_file_with_comments_loads() {
        let (_dir, file) = temp_file();
        fs::create_dir_all(file.path().parent().unwrap()).unwrap();
        fs::write(
            file.path(),
            "# my comment\nversion = 1\n[hotkeys]\nlookup = \"F8\" # inline\n",
        )
        .unwrap();
        let settings = file.load().unwrap().into_settings();
        assert_eq!(settings.hotkeys.lookup, "F8");
        assert_eq!(settings.popup.mode, PopupMode::Responsive);
    }

    #[test]
    fn corrupt_file_is_backed_up_and_replaced_with_defaults() {
        let (_dir, file) = temp_file();
        fs::create_dir_all(file.path().parent().unwrap()).unwrap();
        fs::write(file.path(), "this is [not toml").unwrap();

        let outcome = file.load().unwrap();
        let LoadOutcome::Recovered { settings, backup } = outcome else {
            panic!("expected recovery, got {outcome:?}");
        };
        assert_eq!(settings, Settings::default());
        assert_eq!(fs::read_to_string(&backup).unwrap(), "this is [not toml");
        assert_eq!(
            file.load().unwrap(),
            LoadOutcome::Loaded(Settings::default())
        );
    }

    #[test]
    fn wrong_value_type_counts_as_corrupt() {
        let (_dir, file) = temp_file();
        fs::create_dir_all(file.path().parent().unwrap()).unwrap();
        fs::write(file.path(), "[popup]\nmode = \"sideways\"\n").unwrap();
        assert!(matches!(
            file.load().unwrap(),
            LoadOutcome::Recovered { .. }
        ));
    }

    #[test]
    fn newer_schema_is_loaded_but_never_overwritten() {
        let (_dir, file) = temp_file();
        fs::create_dir_all(file.path().parent().unwrap()).unwrap();
        let newer = format!("version = {}\n", SCHEMA_VERSION + 1);
        fs::write(file.path(), &newer).unwrap();

        let settings = file.load().unwrap().into_settings();
        assert_eq!(settings.version, SCHEMA_VERSION + 1);
        assert!(matches!(
            file.save(&settings),
            Err(ConfigError::NewerVersion { .. })
        ));
        assert_eq!(fs::read_to_string(file.path()).unwrap(), newer);
    }
}
