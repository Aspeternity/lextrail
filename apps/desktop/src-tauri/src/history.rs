//! Lookup history (`ARCHITECTURE.md` 6.3): every lookup that captured English text is
//! recorded in the local database (`lextrail-store`, D037). The database is opened by
//! the first lookup to record, never in the tray-only state (D040).

use std::path::PathBuf;
use std::sync::Mutex;

use lextrail_core::lookup::{CaptureMethod, LookupKind};
use lextrail_store::{LookupOutcome, NewLookup, Store, StoreError};

/// What one lookup adds to the history.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Record {
    pub query: String,
    pub kind: LookupKind,
    /// The base form; `None` for sentences and text the dictionary does not have.
    pub lemma: Option<String>,
    /// The sentence around the selection.
    pub context: Option<String>,
    /// Process name, e.g. `chrome.exe`; never the window title.
    pub source_app: Option<String>,
    pub method: CaptureMethod,
}

pub struct HistoryService {
    /// The data directory (`app_local_data_dir()`); `None` when it is unknown.
    dir: Option<PathBuf>,
    /// `false`: "不记录查询历史" (`history.record` in the settings file).
    enabled: bool,
    store: Mutex<State>,
}

enum State {
    NotOpened,
    Open(Store),
    /// Opening failed; the user was told once, and nothing is recorded until restart.
    Failed,
}

impl HistoryService {
    pub fn new(dir: Option<PathBuf>, enabled: bool) -> Self {
        Self {
            dir,
            enabled,
            store: Mutex::new(State::NotOpened),
        }
    }

    /// Records a lookup, opening the database first if needed. Slow the first time
    /// (opening, migrating): call it after the result is on screen, off the main thread.
    /// When the database cannot be opened `on_open_failure` gets the reason (once; later
    /// lookups are not recorded and do not retry). Returns `None` when nothing was tried.
    pub fn record(
        &self,
        record: &Record,
        on_open_failure: impl FnOnce(&str),
    ) -> Option<LookupOutcome> {
        if !self.enabled {
            return None;
        }
        let mut state = self.store.lock().unwrap_or_else(|e| e.into_inner());
        if matches!(*state, State::NotOpened) {
            *state = match self.open() {
                Ok(store) => State::Open(store),
                Err(e) => {
                    eprintln!("lookup history unavailable: {e}");
                    on_open_failure(&e);
                    State::Failed
                }
            };
        }
        let State::Open(store) = &mut *state else {
            return None;
        };
        let new = NewLookup {
            query: &record.query,
            kind: record.kind,
            lemma: record.lemma.as_deref(),
            context: record.context.as_deref(),
            source_app: record.source_app.as_deref(),
            url: None,
            method: record.method,
        };
        match store.record_lookup(&new) {
            Ok(outcome) => Some(outcome),
            Err(e) => {
                eprintln!("recording a lookup failed: {e}");
                None
            }
        }
    }

    fn open(&self) -> Result<Store, String> {
        let dir = self.dir.as_ref().ok_or("data directory unknown")?;
        Store::open_in_dir(dir).map_err(|e: StoreError| e.to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use lextrail_store::DB_FILE_NAME;

    fn record(query: &str) -> Record {
        Record {
            query: query.into(),
            kind: LookupKind::Word,
            lemma: Some("launch".into()),
            context: Some("They launched it.".into()),
            source_app: Some("chrome.exe".into()),
            method: CaptureMethod::Uia,
        }
    }

    fn no_failure(e: &str) {
        panic!("unexpected failure: {e}");
    }

    #[test]
    fn opens_on_first_record_and_deduplicates() {
        let dir = tempfile::tempdir().unwrap();
        let data = dir.path().join("data");
        let history = HistoryService::new(Some(data.clone()), true);
        // Nothing opened before the first lookup.
        assert!(!data.exists());
        assert!(matches!(
            history.record(&record("launched"), no_failure),
            Some(LookupOutcome::Recorded(_))
        ));
        assert!(data.join(DB_FILE_NAME).is_file());
        // Same query and sentence within 30 s: once.
        assert_eq!(
            history.record(&record("launched"), no_failure),
            Some(LookupOutcome::Repeat)
        );
        assert!(matches!(
            history.record(&record("launch"), no_failure),
            Some(LookupOutcome::Recorded(_))
        ));
        drop(history);

        let store = Store::open_in_dir(&data).unwrap();
        let recent = store.recent_lookups(10).unwrap();
        assert_eq!(recent.len(), 2);
        let first = recent.iter().find(|l| l.query == "launched").unwrap();
        assert_eq!(first.lemma.as_deref(), Some("launch"));
        assert_eq!(first.context.as_deref(), Some("They launched it."));
        assert_eq!(first.source_app.as_deref(), Some("chrome.exe"));
        assert_eq!(first.method, CaptureMethod::Uia);
        assert_eq!(first.url, None);
    }

    #[test]
    fn switched_off_records_nothing_and_opens_nothing() {
        let dir = tempfile::tempdir().unwrap();
        let data = dir.path().join("data");
        let history = HistoryService::new(Some(data.clone()), false);
        assert_eq!(history.record(&record("launched"), no_failure), None);
        assert!(!data.exists());
    }

    #[test]
    fn open_failure_is_reported_once_and_lookups_go_on() {
        let dir = tempfile::tempdir().unwrap();
        // A file where the data directory should be.
        let blocked = dir.path().join("data");
        std::fs::write(&blocked, "").unwrap();
        let history = HistoryService::new(Some(blocked), true);
        let mut reported = Vec::new();
        assert_eq!(
            history.record(&record("launched"), |e| reported.push(e.to_owned())),
            None
        );
        assert_eq!(reported.len(), 1);
        assert_eq!(
            history.record(&record("launch"), |e| reported.push(e.to_owned())),
            None
        );
        assert_eq!(reported.len(), 1);

        let unknown = HistoryService::new(None, true);
        let mut reported = 0;
        assert_eq!(unknown.record(&record("x"), |_| reported += 1), None);
        assert_eq!(reported, 1);
    }
}
