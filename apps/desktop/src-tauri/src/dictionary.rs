//! The built-in dictionary (`lextrail-dict-core.db`, a Tauri resource next to the
//! executable). Opened on the first lookup that captured text, never in the tray-only
//! state, and kept open while the app runs (`docs/specs/m1-popup-dict.md`).

use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Mutex, MutexGuard};

use lextrail_dict::Dictionary;

pub struct DictionaryService {
    /// Where the file should be; `None` when the resource directory is unknown.
    path: Option<PathBuf>,
    dict: Mutex<Option<Dictionary>>,
    /// A failure to open was logged already (the next attempts stay quiet).
    failure_logged: AtomicBool,
}

impl DictionaryService {
    pub fn new(path: Option<PathBuf>) -> Self {
        Self {
            path,
            dict: Mutex::new(None),
            failure_logged: AtomicBool::new(false),
        }
    }

    /// The open dictionary, opening it first if needed. Holds the lock while the caller
    /// uses it (a lookup takes a few milliseconds; only the capture thread uses it). `None`
    /// inside when the file is missing or not a dictionary this build reads; that is
    /// logged once, and opening is tried again on the next lookup (it is cheap).
    pub fn get(&self) -> MutexGuard<'_, Option<Dictionary>> {
        let mut dict = self.dict.lock().unwrap_or_else(|e| e.into_inner());
        if dict.is_none() {
            let opened = match &self.path {
                Some(path) => Dictionary::open(path).map_err(|e| e.to_string()),
                None => Err("resource directory unknown".to_owned()),
            };
            match opened {
                Ok(d) => {
                    *dict = Some(d);
                    self.failure_logged.store(false, Ordering::Relaxed);
                }
                Err(e) => {
                    if !self.failure_logged.swap(true, Ordering::Relaxed) {
                        eprintln!("dictionary unavailable: {e}");
                    }
                }
            }
        }
        dict
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn missing_file_gives_none_and_is_retried() {
        let dir = std::env::temp_dir().join("lextrail-desktop-no-such-dict");
        let service = DictionaryService::new(Some(dir.join("none.db")));
        assert!(service.get().is_none());
        assert!(service.get().is_none());
        assert!(DictionaryService::new(None).get().is_none());
    }
}
