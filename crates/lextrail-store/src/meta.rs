//! The local, never-synced `meta` key/value table.

use lextrail_core::{Hlc, Uuid, parse_canonical_uuid};
use rusqlite::{Connection, OptionalExtension, params};

use crate::StoreError;

const DEVICE_ID: &str = "device_id";
const LAST_HLC: &str = "hlc_last";

fn get(conn: &Connection, key: &str) -> rusqlite::Result<Option<String>> {
    conn.query_row("SELECT value FROM meta WHERE key = ?1", [key], |row| {
        row.get(0)
    })
    .optional()
}

/// This device's ID; generated (UUIDv7) the first time the database is opened.
pub(crate) fn device_id(conn: &Connection) -> Result<Uuid, StoreError> {
    conn.execute(
        "INSERT INTO meta (key, value) VALUES (?1, ?2) ON CONFLICT (key) DO NOTHING",
        params![DEVICE_ID, Uuid::now_v7().to_string()],
    )?;
    let value = get(conn, DEVICE_ID)?.unwrap_or_default();
    parse_canonical_uuid(&value)
        .ok_or_else(|| StoreError::Corrupt(format!("device_id {value:?} in meta")))
}

pub(crate) fn last_hlc(conn: &Connection) -> Result<Option<Hlc>, StoreError> {
    get(conn, LAST_HLC)?
        .map(|value| {
            value
                .parse()
                .map_err(|e| StoreError::Corrupt(format!("{e} in meta")))
        })
        .transpose()
}

pub(crate) fn set_last_hlc(conn: &Connection, hlc: &Hlc) -> rusqlite::Result<()> {
    conn.execute(
        "INSERT INTO meta (key, value) VALUES (?1, ?2)
         ON CONFLICT (key) DO UPDATE SET value = excluded.value",
        params![LAST_HLC, hlc.to_string()],
    )?;
    Ok(())
}
