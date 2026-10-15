//! Lookup history (ARCHITECTURE.md 6.3): insert and soft delete only.

use lextrail_core::lookup::{CaptureMethod, LookupKind, is_repeat, normalize_text};
use lextrail_core::{Clock, Hlc, Uuid, parse_canonical_uuid};
use rusqlite::{OptionalExtension, Row, params};

use crate::{Store, StoreError, sql_millis};

/// A lookup to record. Text is normalized before it is stored.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct NewLookup<'a> {
    pub query: &'a str,
    pub kind: LookupKind,
    pub lemma: Option<&'a str>,
    pub context: Option<&'a str>,
    /// Process name, e.g. `chrome.exe`. Never the window title.
    pub source_app: Option<&'a str>,
    pub url: Option<&'a str>,
    pub method: CaptureMethod,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LookupOutcome {
    Recorded(Uuid),
    /// Same query and context within the dedup window; nothing written.
    Repeat,
    /// The query was empty after normalization; nothing written.
    Empty,
}

/// A stored lookup.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Lookup {
    pub id: Uuid,
    pub created_at: u64,
    pub updated_at: Hlc,
    pub device_id: Uuid,
    pub deleted: bool,
    pub query: String,
    pub kind: LookupKind,
    pub lemma: Option<String>,
    pub context: Option<String>,
    pub source_app: Option<String>,
    pub url: Option<String>,
    pub method: CaptureMethod,
}

const COLUMNS: &str = "id, created_at, updated_at, device_id, deleted, query, kind, lemma, \
                       context, source_app, url, method";

fn non_empty(s: Option<&str>) -> Option<&str> {
    s.map(str::trim).filter(|s| !s.is_empty())
}

fn corrupt(what: &str, value: impl std::fmt::Display) -> StoreError {
    StoreError::Corrupt(format!("lookups.{what} {value}"))
}

fn from_row(row: &Row<'_>) -> Result<Lookup, StoreError> {
    let text = |i: usize| row.get::<_, String>(i);
    let id = text(0)?;
    let created_at: i64 = row.get(1)?;
    let updated_at = text(2)?;
    let device_id = text(3)?;
    let kind = text(6)?;
    let method = text(11)?;
    Ok(Lookup {
        id: parse_canonical_uuid(&id).ok_or_else(|| corrupt("id", &id))?,
        created_at: u64::try_from(created_at).map_err(|_| corrupt("created_at", created_at))?,
        updated_at: updated_at
            .parse()
            .map_err(|_| corrupt("updated_at", &updated_at))?,
        device_id: parse_canonical_uuid(&device_id)
            .ok_or_else(|| corrupt("device_id", &device_id))?,
        deleted: row.get(4)?,
        query: text(5)?,
        kind: kind.parse().map_err(|_| corrupt("kind", &kind))?,
        lemma: row.get(7)?,
        context: row.get(8)?,
        source_app: row.get(9)?,
        url: row.get(10)?,
        method: method.parse().map_err(|_| corrupt("method", &method))?,
    })
}

impl<C: Clock> Store<C> {
    /// Records a lookup, unless it is empty or repeats the same query and context within
    /// [`lextrail_core::lookup::DEDUP_WINDOW_MS`].
    pub fn record_lookup(&mut self, new: &NewLookup<'_>) -> Result<LookupOutcome, StoreError> {
        let query = normalize_text(new.query);
        if query.is_empty() {
            return Ok(LookupOutcome::Empty);
        }
        let context = new.context.map(normalize_text).filter(|c| !c.is_empty());

        self.write(|w| {
            let now = w.now_millis();
            let window = sql_millis(lextrail_core::lookup::DEDUP_WINDOW_MS);
            let previous: Option<i64> = w
                .tx()
                .query_row(
                    "SELECT created_at FROM lookups
                     WHERE created_at > ?1 - ?2 AND created_at < ?1 + ?2
                       AND query = ?3 AND context IS ?4 AND deleted = 0
                     ORDER BY created_at DESC LIMIT 1",
                    params![sql_millis(now), window, query, context],
                    |row| row.get(0),
                )
                .optional()?;
            if previous.is_some_and(|p| is_repeat(u64::try_from(p).unwrap_or(0), now)) {
                return Ok(LookupOutcome::Repeat);
            }

            let id = Uuid::now_v7();
            let stamp = w.stamp()?;
            w.tx().execute(
                &format!(
                    "INSERT INTO lookups ({COLUMNS})
                     VALUES (?1, ?2, ?3, ?4, 0, ?5, ?6, ?7, ?8, ?9, ?10, ?11)"
                ),
                params![
                    id.to_string(),
                    sql_millis(now),
                    stamp.to_string(),
                    w.device_id().to_string(),
                    query,
                    new.kind.as_str(),
                    non_empty(new.lemma),
                    context,
                    non_empty(new.source_app),
                    non_empty(new.url),
                    new.method.as_str(),
                ],
            )?;
            Ok(LookupOutcome::Recorded(id))
        })
    }

    /// Soft-deletes a lookup. Returns `false` if it does not exist or is already deleted.
    pub fn delete_lookup(&mut self, id: Uuid) -> Result<bool, StoreError> {
        self.write(|w| {
            let exists = w
                .tx()
                .query_row(
                    "SELECT 1 FROM lookups WHERE id = ?1 AND deleted = 0",
                    [id.to_string()],
                    |_| Ok(()),
                )
                .optional()?
                .is_some();
            if !exists {
                return Ok(false);
            }
            let stamp = w.stamp()?;
            w.tx().execute(
                "UPDATE lookups SET deleted = 1, updated_at = ?2, device_id = ?3 WHERE id = ?1",
                params![id.to_string(), stamp.to_string(), w.device_id().to_string()],
            )?;
            Ok(true)
        })
    }

    pub fn lookup(&self, id: Uuid) -> Result<Option<Lookup>, StoreError> {
        let mut stmt = self
            .conn
            .prepare(&format!("SELECT {COLUMNS} FROM lookups WHERE id = ?1"))?;
        let mut rows = stmt.query([id.to_string()])?;
        rows.next()?.map(from_row).transpose()
    }

    /// The most recent lookups that are not deleted, newest first.
    pub fn recent_lookups(&self, limit: usize) -> Result<Vec<Lookup>, StoreError> {
        let mut stmt = self.conn.prepare(&format!(
            "SELECT {COLUMNS} FROM lookups WHERE deleted = 0
             ORDER BY created_at DESC, updated_at DESC LIMIT ?1"
        ))?;
        let mut rows = stmt.query([i64::try_from(limit).unwrap_or(i64::MAX)])?;
        let mut out = Vec::new();
        while let Some(row) = rows.next()? {
            out.push(from_row(row)?);
        }
        Ok(out)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use lextrail_core::ManualClock;
    use lextrail_core::lookup::DEDUP_WINDOW_MS;

    fn store() -> (ManualClock, Store<ManualClock>) {
        let clock = ManualClock::new(1_760_000_000_000);
        let store = Store::open_in_memory_with_clock(clock.clone()).unwrap();
        (clock, store)
    }

    fn word(query: &str) -> NewLookup<'_> {
        NewLookup {
            query,
            kind: LookupKind::Word,
            lemma: Some("address"),
            context: Some("We must address the root cause."),
            source_app: Some("chrome.exe"),
            url: None,
            method: CaptureMethod::Uia,
        }
    }

    fn recorded(outcome: LookupOutcome) -> Uuid {
        match outcome {
            LookupOutcome::Recorded(id) => id,
            other => panic!("expected Recorded, got {other:?}"),
        }
    }

    #[test]
    fn records_all_fields() {
        let (clock, mut store) = store();
        let new = NewLookup {
            query: "  addressed \n",
            kind: LookupKind::Word,
            lemma: Some("address"),
            context: Some("We  addressed\tit."),
            source_app: Some(" chrome.exe "),
            url: Some(""),
            method: CaptureMethod::ClipboardDoubleCopy,
        };
        let id = recorded(store.record_lookup(&new).unwrap());
        let got = store.lookup(id).unwrap().unwrap();
        assert_eq!(
            got,
            Lookup {
                id,
                created_at: clock.now_millis(),
                updated_at: store.last_hlc().unwrap(),
                device_id: store.device_id(),
                deleted: false,
                query: "addressed".into(),
                kind: LookupKind::Word,
                lemma: Some("address".into()),
                context: Some("We addressed it.".into()),
                source_app: Some("chrome.exe".into()),
                url: None,
                method: CaptureMethod::ClipboardDoubleCopy,
            }
        );
        assert_eq!(got.id.get_version_num(), 7);
        assert_eq!(got.updated_at.node(), store.device_id());
        assert_eq!(
            crate::meta::last_hlc(&store.conn).unwrap(),
            Some(got.updated_at)
        );
    }

    #[test]
    fn empty_query_is_not_recorded() {
        let (_clock, mut store) = store();
        assert_eq!(
            store.record_lookup(&word(" \n\t ")).unwrap(),
            LookupOutcome::Empty
        );
        assert!(store.recent_lookups(10).unwrap().is_empty());
        assert_eq!(store.last_hlc(), None);
    }

    #[test]
    fn repeat_within_window_is_recorded_once() {
        let (clock, mut store) = store();
        recorded(store.record_lookup(&word("address")).unwrap());
        let hlc = store.last_hlc();

        clock.advance(DEDUP_WINDOW_MS - 1);
        // Whitespace differences still count as the same query.
        assert_eq!(
            store.record_lookup(&word(" address ")).unwrap(),
            LookupOutcome::Repeat
        );
        assert_eq!(store.last_hlc(), hlc);
        assert_eq!(store.recent_lookups(10).unwrap().len(), 1);

        clock.advance(1);
        recorded(store.record_lookup(&word("address")).unwrap());
        assert_eq!(store.recent_lookups(10).unwrap().len(), 2);
    }

    #[test]
    fn different_context_or_query_is_not_a_repeat() {
        let (_clock, mut store) = store();
        recorded(store.record_lookup(&word("address")).unwrap());
        let other_context = NewLookup {
            context: Some("Send it to this address."),
            ..word("address")
        };
        recorded(store.record_lookup(&other_context).unwrap());
        let no_context = NewLookup {
            context: None,
            ..word("address")
        };
        recorded(store.record_lookup(&no_context).unwrap());
        assert_eq!(
            store.record_lookup(&no_context).unwrap(),
            LookupOutcome::Repeat
        );
        recorded(store.record_lookup(&word("addresses")).unwrap());
        assert_eq!(store.recent_lookups(10).unwrap().len(), 4);
    }

    #[test]
    fn deleted_lookup_does_not_suppress_a_new_one() {
        let (_clock, mut store) = store();
        let id = recorded(store.record_lookup(&word("address")).unwrap());
        assert!(store.delete_lookup(id).unwrap());
        recorded(store.record_lookup(&word("address")).unwrap());
    }

    #[test]
    fn soft_delete_stamps_a_newer_hlc() {
        let (_clock, mut store) = store();
        let id = recorded(store.record_lookup(&word("address")).unwrap());
        let created = store.lookup(id).unwrap().unwrap().updated_at;

        assert!(store.delete_lookup(id).unwrap());
        let deleted = store.lookup(id).unwrap().unwrap();
        assert!(deleted.deleted);
        assert!(deleted.updated_at > created);
        assert!(store.recent_lookups(10).unwrap().is_empty());

        assert!(!store.delete_lookup(id).unwrap());
        assert!(!store.delete_lookup(Uuid::now_v7()).unwrap());
        assert_eq!(store.lookup(id).unwrap().unwrap(), deleted);
    }

    #[test]
    fn recent_lookups_are_newest_first_and_limited() {
        let (clock, mut store) = store();
        for q in ["one", "two", "three"] {
            recorded(store.record_lookup(&word(q)).unwrap());
            clock.advance(1);
        }
        let queries: Vec<String> = store
            .recent_lookups(2)
            .unwrap()
            .into_iter()
            .map(|l| l.query)
            .collect();
        assert_eq!(queries, ["three", "two"]);
    }

    #[test]
    fn check_constraints_reject_unknown_values() {
        let (_clock, store) = store();
        let insert = |kind: &str, method: &str, deleted: i64| {
            store.conn.execute(
                "INSERT INTO lookups (id, created_at, updated_at, device_id, deleted, query, kind, method)
                 VALUES (?1, 0, 'x', 'x', ?2, 'q', ?3, ?4)",
                params![Uuid::now_v7().to_string(), deleted, kind, method],
            )
        };
        assert!(insert("word", "uia", 0).is_ok());
        assert!(insert("Word", "uia", 0).is_err());
        assert!(insert("idiom", "uia", 0).is_err());
        assert!(insert("word", "UIA", 0).is_err());
        assert!(insert("word", "screenshot", 0).is_err());
        assert!(insert("word", "uia", 2).is_err());
        // Every enum value passes its constraint.
        for k in LookupKind::ALL {
            for m in CaptureMethod::ALL {
                assert!(insert(k.as_str(), m.as_str(), 0).is_ok(), "{k} {m}");
            }
        }
    }
}
