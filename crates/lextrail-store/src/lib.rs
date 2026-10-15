//! LexTrail local database (D037): SQLite storage and schema migrations.
//!
//! - The database file is [`DB_FILE_NAME`] in the app's local (non-roaming) data directory,
//!   `%LOCALAPPDATA%\io.github.aspeternity.lextrail\` on Windows (D040). The caller passes
//!   the directory in.
//! - Open it on demand, when a feature first needs it; the tray-only idle state keeps no
//!   database open.
//! - Every write that stamps records with an HLC timestamp also saves the clock's last value
//!   to the `meta` table in the same transaction (ARCHITECTURE.md 6.2).

mod lookups;
mod meta;
mod schema;

use std::fs;
use std::path::{Path, PathBuf};
use std::time::Duration;

use lextrail_core::{Clock, Hlc, HlcClock, HlcError, SystemClock, Uuid};
use rusqlite::{Connection, Transaction};

pub use lookups::{Lookup, LookupOutcome, NewLookup};
pub use schema::SCHEMA_VERSION;

/// File name of the database inside the data directory.
pub const DB_FILE_NAME: &str = "lextrail.db";

/// How long a statement waits for a lock held by another connection.
pub const BUSY_TIMEOUT: Duration = Duration::from_secs(5);

#[derive(Debug, thiserror::Error)]
pub enum StoreError {
    #[error("failed to create data directory {path}: {source}")]
    CreateDir {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
    #[error(
        "database was written by a newer LexTrail (schema {found}, this build supports \
         {SCHEMA_VERSION}); refusing to open it"
    )]
    NewerVersion { found: i64 },
    #[error("database contains invalid data: {0}")]
    Corrupt(String),
    #[error(transparent)]
    Sqlite(#[from] rusqlite::Error),
    #[error(transparent)]
    Hlc(#[from] HlcError),
}

/// An open local database.
#[derive(Debug)]
pub struct Store<C: Clock = SystemClock> {
    conn: Connection,
    hlc: HlcClock<C>,
}

impl Store<SystemClock> {
    /// Opens (creating if needed) the database in `dir`.
    pub fn open_in_dir(dir: impl AsRef<Path>) -> Result<Self, StoreError> {
        Self::open_in_dir_with_clock(dir, SystemClock)
    }
}

impl<C: Clock> Store<C> {
    pub fn open_in_dir_with_clock(dir: impl AsRef<Path>, clock: C) -> Result<Self, StoreError> {
        let dir = dir.as_ref();
        fs::create_dir_all(dir).map_err(|source| StoreError::CreateDir {
            path: dir.to_owned(),
            source,
        })?;
        Self::init(Connection::open(dir.join(DB_FILE_NAME))?, clock)
    }

    /// An in-memory database, for tests.
    pub fn open_in_memory_with_clock(clock: C) -> Result<Self, StoreError> {
        Self::init(Connection::open_in_memory()?, clock)
    }

    fn init(mut conn: Connection, clock: C) -> Result<Self, StoreError> {
        // Connection-level settings only; nothing is written to the file yet.
        conn.busy_timeout(BUSY_TIMEOUT)?;
        conn.pragma_update(None, "foreign_keys", true)?;
        // Check before switching to WAL, which would change a newer build's file.
        schema::check_version(schema::user_version(&conn)?)?;
        // Returns the resulting mode ("memory" for in-memory databases).
        conn.query_row("PRAGMA journal_mode = WAL", [], |row| {
            row.get::<_, String>(0)
        })?;
        conn.pragma_update(None, "synchronous", "NORMAL")?;
        schema::migrate(&mut conn)?;

        let device_id = meta::device_id(&conn)?;
        let last = meta::last_hlc(&conn)?;
        Ok(Self {
            conn,
            hlc: HlcClock::new(clock, device_id, last),
        })
    }

    /// This device's ID, generated when the database was created.
    pub fn device_id(&self) -> Uuid {
        self.hlc.node()
    }

    /// The last HLC timestamp issued on this device.
    pub fn last_hlc(&self) -> Option<Hlc> {
        self.hlc.last()
    }

    /// Runs `f` in one transaction. If it stamped anything, the clock's last value is saved
    /// in the same transaction; on any error both the data and the clock are rolled back.
    pub(crate) fn write<T>(
        &mut self,
        f: impl FnOnce(&mut Write<'_, C>) -> Result<T, StoreError>,
    ) -> Result<T, StoreError> {
        let before = self.hlc.last();
        let result = (|| {
            let mut w = Write {
                tx: self.conn.transaction()?,
                hlc: &mut self.hlc,
                stamped: false,
            };
            let value = f(&mut w)?;
            if w.stamped
                && let Some(last) = w.hlc.last()
            {
                meta::set_last_hlc(&w.tx, &last)?;
            }
            w.tx.commit()?;
            Ok(value)
        })();
        if result.is_err() {
            self.hlc.restore(before);
        }
        result
    }
}

/// A write transaction with access to the HLC clock.
pub(crate) struct Write<'a, C> {
    tx: Transaction<'a>,
    hlc: &'a mut HlcClock<C>,
    stamped: bool,
}

impl<C: Clock> Write<'_, C> {
    pub(crate) fn tx(&self) -> &Transaction<'_> {
        &self.tx
    }

    /// A new HLC timestamp for a local change.
    pub(crate) fn stamp(&mut self) -> Result<Hlc, StoreError> {
        let hlc = self.hlc.now()?;
        self.stamped = true;
        Ok(hlc)
    }

    /// Wall-clock time in Unix milliseconds, for `created_at`.
    pub(crate) fn now_millis(&self) -> u64 {
        self.hlc.clock().now_millis()
    }

    pub(crate) fn device_id(&self) -> Uuid {
        self.hlc.node()
    }
}

/// Converts Unix milliseconds for an `INTEGER` column.
pub(crate) fn sql_millis(millis: u64) -> i64 {
    i64::try_from(millis).unwrap_or(i64::MAX)
}

#[cfg(test)]
mod tests {
    use super::*;
    use lextrail_core::{ManualClock, parse_canonical_uuid};
    use rusqlite::OptionalExtension;

    fn pragma<T: rusqlite::types::FromSql>(store: &Store<ManualClock>, name: &str) -> T {
        store
            .conn
            .pragma_query_value(None, name, |row| row.get(0))
            .unwrap()
    }

    #[test]
    fn in_memory_store_is_migrated() {
        let store = Store::open_in_memory_with_clock(ManualClock::new(1000)).unwrap();
        assert_eq!(schema::user_version(&store.conn).unwrap(), SCHEMA_VERSION);
        assert_eq!(store.last_hlc(), None);
    }

    #[test]
    fn file_store_uses_wal_and_connection_settings() {
        let dir = tempfile::tempdir().unwrap();
        let data_dir = dir.path().join("nested").join("data");
        let store = Store::open_in_dir_with_clock(&data_dir, ManualClock::new(1000)).unwrap();
        assert!(data_dir.join(DB_FILE_NAME).is_file());
        assert_eq!(pragma::<String>(&store, "journal_mode"), "wal");
        assert_eq!(pragma::<i64>(&store, "synchronous"), 1); // NORMAL
        assert_eq!(pragma::<i64>(&store, "foreign_keys"), 1);
        assert_eq!(
            pragma::<i64>(&store, "busy_timeout"),
            BUSY_TIMEOUT.as_millis() as i64
        );
    }

    #[test]
    fn device_id_is_created_once_and_kept() {
        let dir = tempfile::tempdir().unwrap();
        let first = Store::open_in_dir_with_clock(dir.path(), ManualClock::new(1000))
            .unwrap()
            .device_id();
        assert_eq!(first.get_version_num(), 7);
        assert!(parse_canonical_uuid(&first.to_string()).is_some());
        let again = Store::open_in_dir_with_clock(dir.path(), ManualClock::new(1000))
            .unwrap()
            .device_id();
        assert_eq!(first, again);
    }

    #[test]
    fn refuses_newer_database_without_modifying_it() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join(DB_FILE_NAME);
        {
            let conn = Connection::open(&path).unwrap();
            conn.execute_batch("CREATE TABLE future (x INTEGER);")
                .unwrap();
            conn.pragma_update(None, "user_version", SCHEMA_VERSION + 1)
                .unwrap();
        }
        let before = fs::read(&path).unwrap();

        let err = Store::open_in_dir_with_clock(dir.path(), ManualClock::new(1000)).unwrap_err();
        assert!(
            matches!(err, StoreError::NewerVersion { found } if found == SCHEMA_VERSION + 1),
            "{err}"
        );

        assert_eq!(fs::read(&path).unwrap(), before);
        assert!(!dir.path().join(format!("{DB_FILE_NAME}-wal")).exists());
        let conn = Connection::open(&path).unwrap();
        let mode: String = conn
            .pragma_query_value(None, "journal_mode", |row| row.get(0))
            .unwrap();
        assert_eq!(mode, "delete");
    }

    #[test]
    fn invalid_device_id_is_reported_as_corrupt() {
        let dir = tempfile::tempdir().unwrap();
        drop(Store::open_in_dir_with_clock(dir.path(), ManualClock::new(1000)).unwrap());
        let conn = Connection::open(dir.path().join(DB_FILE_NAME)).unwrap();
        conn.execute(
            "UPDATE meta SET value = upper(value) WHERE key = 'device_id'",
            [],
        )
        .unwrap();
        drop(conn);
        let err = Store::open_in_dir_with_clock(dir.path(), ManualClock::new(1000)).unwrap_err();
        assert!(matches!(err, StoreError::Corrupt(_)), "{err}");
    }

    #[test]
    fn stamped_write_saves_last_hlc_in_the_same_transaction() {
        let mut store = Store::open_in_memory_with_clock(ManualClock::new(1000)).unwrap();
        let hlc = store.write(|w| w.stamp()).unwrap();
        assert_eq!(store.last_hlc(), Some(hlc));
        assert_eq!(meta::last_hlc(&store.conn).unwrap(), Some(hlc));
        assert_eq!(hlc.node(), store.device_id());
    }

    #[test]
    fn failed_write_rolls_back_data_and_clock() {
        let mut store = Store::open_in_memory_with_clock(ManualClock::new(1000)).unwrap();
        let first = store.write(|w| w.stamp()).unwrap();

        let err = store
            .write(|w| {
                w.stamp()?;
                w.tx()
                    .execute("INSERT INTO meta (key, value) VALUES ('probe', 'x')", [])?;
                Err::<(), _>(StoreError::Corrupt("simulated failure".into()))
            })
            .unwrap_err();
        assert!(matches!(err, StoreError::Corrupt(_)));

        assert_eq!(store.last_hlc(), Some(first));
        assert_eq!(meta::last_hlc(&store.conn).unwrap(), Some(first));
        let probe: Option<String> = store
            .conn
            .query_row("SELECT value FROM meta WHERE key = 'probe'", [], |row| {
                row.get(0)
            })
            .optional()
            .unwrap();
        assert_eq!(probe, None);
    }

    #[test]
    fn hlc_stays_monotonic_across_reopen_with_clock_moved_back() {
        let dir = tempfile::tempdir().unwrap();
        let first = {
            let mut store =
                Store::open_in_dir_with_clock(dir.path(), ManualClock::new(50_000)).unwrap();
            store.write(|w| w.stamp()).unwrap()
        };
        let mut store = Store::open_in_dir_with_clock(dir.path(), ManualClock::new(1000)).unwrap();
        assert_eq!(store.last_hlc(), Some(first));
        let next = store.write(|w| w.stamp()).unwrap();
        assert!(next > first);
        assert_eq!((next.millis(), next.counter()), (50_000, 1));
    }
}
