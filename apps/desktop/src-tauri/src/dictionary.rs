//! The built-in dictionary (`lextrail-dict-core.db`, a Tauri resource next to the
//! executable) and the extended (Oxford) dictionary (`docs/specs/m1.5-oxford.md` 2). Both
//! are opened on the first lookup that captured text, never in the tray-only state, and
//! kept open while the app runs (`docs/specs/m1-popup-dict.md`).

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Mutex, MutexGuard};

use lextrail_config::DictionarySettings;
use lextrail_dict::Dictionary;
use lextrail_dict::ext::OXFORD_FILE_NAME;
use lextrail_dict::ext::reader::ExtDictionary;
use lextrail_dict::ext::status::{ExtStatus, oxford_status, unusable_reason};

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

/// The Oxford dictionary in the extended dictionary folder. A missing folder or file, or a
/// file that cannot be used, means "no Oxford data": lookups are as in M1, with no message
/// (the settings window shows why, spec 2.3).
pub struct ExtDictionaryService {
    /// The folder when the settings leave it empty; `None` when the data directory is
    /// unknown.
    default_folder: Option<PathBuf>,
    inner: Mutex<ExtInner>,
}

pub struct ExtInner {
    folder: Option<PathBuf>,
    enabled: bool,
    dict: Option<ExtDictionary>,
}

impl ExtInner {
    /// The open Oxford dictionary, when it is turned on and usable.
    #[cfg_attr(
        not(test),
        expect(
            dead_code,
            reason = "the popup looks words up in it from step 5 (spec 13)"
        )
    )]
    pub fn dictionary(&self) -> Option<&ExtDictionary> {
        self.dict.as_ref()
    }
}

impl ExtDictionaryService {
    pub fn new(default_folder: Option<PathBuf>, settings: &DictionarySettings) -> Self {
        let service = Self {
            default_folder,
            inner: Mutex::new(ExtInner {
                folder: None,
                enabled: false,
                dict: None,
            }),
        };
        service.configure(settings);
        service
    }

    /// The folder `settings` mean: theirs, or the default one when empty.
    pub fn folder_for(&self, settings: &DictionarySettings) -> Option<PathBuf> {
        if settings.folder.is_empty() {
            self.default_folder.clone()
        } else {
            Some(PathBuf::from(&settings.folder))
        }
    }

    pub fn default_folder(&self) -> Option<&Path> {
        self.default_folder.as_deref()
    }

    /// New settings take effect at once: the open file is closed, and the next lookup
    /// opens the one the settings now name (spec 2.3).
    pub fn configure(&self, settings: &DictionarySettings) {
        let mut inner = self.lock();
        inner.folder = self.folder_for(settings);
        inner.enabled = settings.oxford;
        inner.dict = None;
    }

    fn lock(&self) -> MutexGuard<'_, ExtInner> {
        self.inner.lock().unwrap_or_else(|e| e.into_inner())
    }

    /// For a lookup: opens the dictionary first if it is turned on and not open yet.
    /// Failing to open is tried again on the next lookup (the user may fix the folder).
    #[cfg_attr(
        not(test),
        expect(
            dead_code,
            reason = "the popup looks words up in it from step 5 (spec 13)"
        )
    )]
    pub fn get(&self) -> MutexGuard<'_, ExtInner> {
        let mut inner = self.lock();
        if inner.enabled && inner.dict.is_none() {
            let opened = inner
                .folder
                .as_ref()
                .map(|folder| ExtDictionary::open_oxford(&folder.join(OXFORD_FILE_NAME)));
            match opened {
                Some(Ok(dict)) => inner.dict = Some(dict),
                Some(Err(_e)) => {
                    #[cfg(debug_assertions)]
                    eprintln!("oxford dictionary not used: {_e}");
                }
                None => {}
            }
        }
        inner
    }

    /// The status for the settings window, whether or not the dictionary is turned on,
    /// with the folder it is about.
    pub fn status(&self) -> (Option<PathBuf>, ExtStatus) {
        let inner = self.lock();
        let status = match (&inner.dict, &inner.folder) {
            (Some(dict), _) => match dict.info() {
                Ok(info) => ExtStatus::Ready(info),
                Err(e) => ExtStatus::Unusable(unusable_reason(&e)),
            },
            (None, Some(folder)) => oxford_status(folder),
            (None, None) => ExtStatus::FolderMissing,
        };
        (inner.folder.clone(), status)
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

    /// An Oxford dictionary file with a made-up entry count.
    fn oxford(dir: &Path) {
        use lextrail_dict::ext::schema::{FORMAT, FORMAT_VERSION, SCHEMA};
        let conn = rusqlite::Connection::open(dir.join(OXFORD_FILE_NAME)).unwrap();
        conn.execute_batch(SCHEMA).unwrap();
        conn.execute_batch(&format!(
            "INSERT INTO meta VALUES ('format', '{FORMAT}'), ('format_version', '{FORMAT_VERSION}'),
             ('source', 'oalecd10'), ('entry_count', '2');"
        ))
        .unwrap();
    }

    #[test]
    fn the_oxford_dictionary_follows_the_settings() {
        let dir = tempfile::tempdir().unwrap();
        let default = dir.path().join("dicts");
        let service =
            ExtDictionaryService::new(Some(default.clone()), &DictionarySettings::default());
        // The default folder does not exist yet: no Oxford data, quietly.
        assert!(service.get().dictionary().is_none());
        assert_eq!(
            service.status(),
            (Some(default.clone()), ExtStatus::FolderMissing)
        );

        std::fs::create_dir(&default).unwrap();
        assert_eq!(service.status().1, ExtStatus::FileMissing);
        oxford(&default);
        assert!(service.get().dictionary().is_some());
        assert!(matches!(service.status().1, ExtStatus::Ready(i) if i.entries == 2));

        // Turned off: closed at once and not opened by lookups; the status still shows.
        let off = DictionarySettings {
            oxford: false,
            ..DictionarySettings::default()
        };
        service.configure(&off);
        assert!(service.get().dictionary().is_none());
        assert!(matches!(service.status().1, ExtStatus::Ready(_)));

        // Another folder, which roamed in from another computer.
        let elsewhere = DictionarySettings {
            folder: dir.path().join("elsewhere").to_string_lossy().into_owned(),
            ..DictionarySettings::default()
        };
        service.configure(&elsewhere);
        assert!(service.get().dictionary().is_none());
        assert_eq!(service.status().1, ExtStatus::FolderMissing);
        assert_eq!(
            service.folder_for(&elsewhere),
            Some(dir.path().join("elsewhere"))
        );
    }
}
