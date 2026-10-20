//! Schema migrations, tracked with SQLite's `user_version`.
//!
//! Migration `n` (1-based) upgrades the schema from version `n - 1` to `n`. Migrations are
//! append-only: never edit one that has been released.

use rusqlite::{Connection, TransactionBehavior};

use crate::StoreError;

const MIGRATIONS: &[&str] = &[
    include_str!("migrations/0001_init.sql"),
    include_str!("migrations/0002_drop_lookup_source.sql"),
];

/// The schema version this build creates and understands.
pub const SCHEMA_VERSION: i64 = MIGRATIONS.len() as i64;

pub(crate) fn user_version(conn: &Connection) -> rusqlite::Result<i64> {
    conn.pragma_query_value(None, "user_version", |row| row.get(0))
}

/// Fails without touching the database if it was written by a newer build.
pub(crate) fn check_version(version: i64) -> Result<(), StoreError> {
    if version > SCHEMA_VERSION {
        return Err(StoreError::NewerVersion { found: version });
    }
    if version < 0 {
        return Err(StoreError::Corrupt(format!("schema version {version}")));
    }
    Ok(())
}

pub(crate) fn migrate(conn: &mut Connection) -> Result<(), StoreError> {
    migrate_to(conn, SCHEMA_VERSION)
}

/// Applies migrations up to `target`, each in its own transaction together with the
/// `user_version` bump, so a failure leaves the database at the last complete version.
pub(crate) fn migrate_to(conn: &mut Connection, target: i64) -> Result<(), StoreError> {
    debug_assert!((0..=SCHEMA_VERSION).contains(&target));
    loop {
        let tx = conn.transaction_with_behavior(TransactionBehavior::Immediate)?;
        // Re-read inside the write lock in case another connection migrated meanwhile.
        let current = user_version(&tx)?;
        check_version(current)?;
        if current >= target {
            return Ok(());
        }
        tx.execute_batch(MIGRATIONS[current as usize])?;
        tx.pragma_update(None, "user_version", current + 1)?;
        tx.commit()?;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn schema_sql(conn: &Connection) -> Vec<String> {
        let mut stmt = conn
            .prepare("SELECT sql FROM sqlite_schema WHERE sql IS NOT NULL ORDER BY name")
            .unwrap();
        stmt.query_map([], |row| row.get(0))
            .unwrap()
            .collect::<rusqlite::Result<_>>()
            .unwrap()
    }

    #[test]
    fn migrates_from_zero_to_latest() {
        let mut conn = Connection::open_in_memory().unwrap();
        assert_eq!(user_version(&conn).unwrap(), 0);
        migrate(&mut conn).unwrap();
        assert_eq!(user_version(&conn).unwrap(), SCHEMA_VERSION);
        let names: Vec<String> = conn
            .prepare("SELECT name FROM sqlite_schema WHERE type = 'table' ORDER BY name")
            .unwrap()
            .query_map([], |row| row.get(0))
            .unwrap()
            .collect::<rusqlite::Result<_>>()
            .unwrap();
        assert_eq!(names, ["lookups", "meta"]);
    }

    #[test]
    fn every_intermediate_version_upgrades_to_the_same_schema() {
        let mut direct = Connection::open_in_memory().unwrap();
        migrate(&mut direct).unwrap();
        let expected = schema_sql(&direct);

        for start in 0..=SCHEMA_VERSION {
            let mut conn = Connection::open_in_memory().unwrap();
            migrate_to(&mut conn, start).unwrap();
            assert_eq!(user_version(&conn).unwrap(), start);
            migrate(&mut conn).unwrap();
            assert_eq!(user_version(&conn).unwrap(), SCHEMA_VERSION);
            assert_eq!(schema_sql(&conn), expected, "starting from version {start}");
        }
    }

    fn lookup_columns(conn: &Connection) -> Vec<String> {
        conn.prepare("SELECT name FROM pragma_table_info('lookups') ORDER BY cid")
            .unwrap()
            .query_map([], |row| row.get(0))
            .unwrap()
            .collect::<rusqlite::Result<_>>()
            .unwrap()
    }

    #[test]
    fn version_2_drops_where_a_lookup_came_from_and_keeps_the_rows() {
        let mut conn = Connection::open_in_memory().unwrap();
        migrate_to(&mut conn, 1).unwrap();
        conn.execute(
            "INSERT INTO lookups (id, created_at, updated_at, device_id, deleted, query, kind,
                                  lemma, context, source_app, url, method)
             VALUES ('a', 1, 'h', 'd', 0, 'address', 'word', 'address', 'We address it.',
                     'chrome.exe', 'https://example.com/', 'uia')",
            [],
        )
        .unwrap();
        migrate_to(&mut conn, 2).unwrap();
        let columns = lookup_columns(&conn);
        assert!(!columns.iter().any(|c| c == "source_app" || c == "url"));
        let row: (String, String, String) = conn
            .query_row("SELECT query, context, method FROM lookups", [], |row| {
                Ok((row.get(0)?, row.get(1)?, row.get(2)?))
            })
            .unwrap();
        assert_eq!(
            row,
            ("address".into(), "We address it.".into(), "uia".into())
        );
    }

    #[test]
    fn migrating_twice_is_a_no_op() {
        let mut conn = Connection::open_in_memory().unwrap();
        migrate(&mut conn).unwrap();
        migrate(&mut conn).unwrap();
        assert_eq!(user_version(&conn).unwrap(), SCHEMA_VERSION);
    }

    #[test]
    fn refuses_newer_and_negative_versions() {
        assert!(matches!(
            check_version(SCHEMA_VERSION + 1),
            Err(StoreError::NewerVersion { found }) if found == SCHEMA_VERSION + 1
        ));
        assert!(matches!(check_version(-1), Err(StoreError::Corrupt(_))));
        assert!(check_version(0).is_ok());
        assert!(check_version(SCHEMA_VERSION).is_ok());
    }
}
