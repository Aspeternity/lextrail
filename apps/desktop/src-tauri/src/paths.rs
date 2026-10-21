//! Where the settings file and the local database live. Automated runs never use the
//! user's own: debug builds take `LEXTRAIL_DEBUG_CONFIG_DIR` and `LEXTRAIL_DEBUG_DATA_DIR`
//! instead, and the benchmark (`LEXTRAIL_BENCH_TIMINGS`, release builds too) uses scratch
//! directories for both, so its results never depend on the user's settings.

use std::path::PathBuf;

use tauri::{AppHandle, Manager, Runtime};

/// The settings directory (`app_config_dir()`, roaming, D035).
pub fn config_dir<R: Runtime>(app: &AppHandle<R>) -> tauri::Result<PathBuf> {
    if std::env::var_os(crate::popup::timing::ENV).is_some() {
        return Ok(std::env::temp_dir().join("lextrail-bench-config"));
    }
    match debug_override("LEXTRAIL_DEBUG_CONFIG_DIR") {
        Some(dir) => Ok(dir),
        None => app.path().app_config_dir(),
    }
}

/// The data directory for the local database (`app_local_data_dir()`, D040); `None`
/// when it is unknown. `benchmark`: a benchmark run.
pub fn data_dir<R: Runtime>(app: &AppHandle<R>, benchmark: bool) -> Option<PathBuf> {
    if benchmark {
        return Some(std::env::temp_dir().join("lextrail-bench-data"));
    }
    debug_override("LEXTRAIL_DEBUG_DATA_DIR").or_else(|| app.path().app_local_data_dir().ok())
}

/// A non-empty directory from `var`, in debug builds only.
fn debug_override(var: &str) -> Option<PathBuf> {
    if !cfg!(debug_assertions) {
        return None;
    }
    std::env::var_os(var)
        .filter(|v| !v.is_empty())
        .map(PathBuf::from)
}
