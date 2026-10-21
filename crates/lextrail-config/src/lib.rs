//! LexTrail local settings and secret storage.
//!
//! - [`Settings`] is the user-editable configuration, stored as TOML by [`SettingsFile`].
//! - [`hotkey`] parses hotkey strings and decides which combinations may be used.
//! - Secrets (AI API keys, sync tokens) never go into the settings file (D032); they are
//!   kept in the OS credential manager through a [`SecretStore`] adapter (D007).

mod file;
pub mod hotkey;
mod secrets;
mod settings;

pub use file::{ConfigError, FileStamp, LoadOutcome, SettingsFile};
#[cfg(windows)]
pub use secrets::WindowsCredentialStore;
pub use secrets::{MemorySecretStore, SecretError, SecretKey, SecretStore};
pub use settings::{
    HistorySettings, HotkeySettings, PopupMode, PopupSettings, SCHEMA_VERSION, Settings,
};
