//! The settings schema and its defaults.

use serde::{Deserialize, Serialize};

/// Version of the settings schema written by this build. Bump it when a change needs a
/// migration; purely additive fields don't, because missing fields fall back to defaults.
pub const SCHEMA_VERSION: u32 = 1;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct Settings {
    pub version: u32,
    pub hotkeys: HotkeySettings,
    pub popup: PopupSettings,
    pub history: HistorySettings,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            version: SCHEMA_VERSION,
            hotkeys: HotkeySettings::default(),
            popup: PopupSettings::default(),
            history: HistorySettings::default(),
        }
    }
}

/// Global hotkeys as accelerator strings, e.g. `"Ctrl+Alt+X"`. Validation happens where
/// they are registered, so an invalid value is reported to the user instead of
/// making the whole file unreadable.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct HotkeySettings {
    /// Capture the selection and show the popup. Provisional default per D014.
    pub lookup: String,
}

impl Default for HotkeySettings {
    fn default() -> Self {
        Self {
            lookup: "Ctrl+Alt+X".to_owned(),
        }
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct PopupSettings {
    pub mode: PopupMode,
}

/// Popup lifecycle preference (D009).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PopupMode {
    /// Keep the hidden popup suspended so it reappears quickly (default).
    #[default]
    Responsive,
    /// Destroy the popup after each use to free memory.
    MemorySaver,
}

/// Lookup history (`ARCHITECTURE.md` 6.3).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct HistorySettings {
    /// Record lookups in the local database; `false` is the "不记录查询历史" switch.
    pub record: bool,
}

impl Default for HistorySettings {
    fn default() -> Self {
        Self { record: true }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_match_decisions() {
        let settings = Settings::default();
        assert_eq!(settings.version, SCHEMA_VERSION);
        assert_eq!(settings.hotkeys.lookup, "Ctrl+Alt+X"); // D014
        assert_eq!(settings.popup.mode, PopupMode::Responsive); // D009
        assert!(settings.history.record);
    }

    #[test]
    fn toml_round_trip() {
        let settings = Settings {
            popup: PopupSettings {
                mode: PopupMode::MemorySaver,
            },
            hotkeys: HotkeySettings {
                lookup: "Ctrl+Shift+F1".into(),
            },
            history: HistorySettings { record: false },
            ..Settings::default()
        };
        let text = toml::to_string(&settings).unwrap();
        assert_eq!(toml::from_str::<Settings>(&text).unwrap(), settings);
    }

    #[test]
    fn missing_fields_fall_back_to_defaults() {
        let settings: Settings = toml::from_str("[popup]\nmode = \"memory_saver\"\n").unwrap();
        assert_eq!(settings.popup.mode, PopupMode::MemorySaver);
        assert_eq!(settings.hotkeys, HotkeySettings::default());
        assert!(settings.history.record);
        assert_eq!(settings.version, SCHEMA_VERSION);
    }

    #[test]
    fn empty_file_is_all_defaults() {
        assert_eq!(toml::from_str::<Settings>("").unwrap(), Settings::default());
    }

    #[test]
    fn unknown_fields_are_ignored() {
        let settings: Settings =
            toml::from_str("future_option = true\n[hotkeys]\nlookup = \"F8\"\nother = 1\n")
                .unwrap();
        assert_eq!(settings.hotkeys.lookup, "F8");
    }

    #[test]
    fn popup_mode_uses_snake_case() {
        let text = toml::to_string(&Settings::default()).unwrap();
        assert!(text.contains("mode = \"responsive\""), "{text}");
    }
}
