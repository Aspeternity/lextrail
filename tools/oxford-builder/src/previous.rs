//! Maps the sense and group ids of a previous conversion to the new ones
//! (`docs/specs/m1.5-oxford.md` 5.4), by fingerprint.

use std::collections::HashMap;
use std::path::Path;

use anyhow::{Context, Result, bail};
use lextrail_dict::ext::OXFORD_SOURCE;
use lextrail_dict::ext::schema::FORMAT;
use rusqlite::{Connection, OpenFlags, params};

/// A sense or (non-main) group of a conversion.
#[derive(Debug, Clone)]
pub struct Item {
    pub entry_key: String,
    pub source_entry: i64,
    pub grp: String,
    /// `None` for a group.
    pub sense: Option<i64>,
    pub fingerprint: String,
}

impl Item {
    pub fn id(&self) -> String {
        let group = format!("{OXFORD_SOURCE}/{}/{}", self.entry_key, self.grp);
        match self.sense {
            Some(n) => format!("{group}/{n}"),
            None => group,
        }
    }

    /// Group kind (`s`, `i`, `p`, `d`) and whether it is a sense.
    fn kind(&self) -> (char, bool) {
        (self.grp.chars().next().unwrap_or('s'), self.sense.is_some())
    }
}

#[derive(Debug, Default, Clone, serde::Serialize)]
pub struct RenameCounts {
    pub unchanged: u64,
    pub same_entry: u64,
    pub cross_entry: u64,
    pub removed: u64,
}

/// `(old id, new id, how)`.
pub type Rename = (String, Option<String>, &'static str);

/// Reads the senses and groups of a dictionary file written by this converter.
pub fn read_items(conn: &Connection) -> Result<Vec<Item>> {
    let mut items = Vec::new();
    let mut stmt = conn.prepare(
        "SELECT e.entry_key, e.source_entry, s.grp, s.sense, s.fingerprint
         FROM senses s JOIN entries e ON e.id = s.entry_id
         ORDER BY e.id, s.grp, s.sense",
    )?;
    for row in stmt.query_map([], |r| {
        Ok(Item {
            entry_key: r.get(0)?,
            source_entry: r.get(1)?,
            grp: r.get(2)?,
            sense: Some(r.get(3)?),
            fingerprint: r.get(4)?,
        })
    })? {
        items.push(row?);
    }
    let mut stmt = conn.prepare(
        "SELECT e.entry_key, e.source_entry, g.grp, g.fingerprint
         FROM groups g JOIN entries e ON e.id = g.entry_id
         WHERE g.fingerprint IS NOT NULL ORDER BY e.id, g.ord",
    )?;
    for row in stmt.query_map([], |r| {
        Ok(Item {
            entry_key: r.get(0)?,
            source_entry: r.get(1)?,
            grp: r.get(2)?,
            sense: None,
            fingerprint: r.get(3)?,
        })
    })? {
        items.push(row?);
    }
    Ok(items)
}

pub fn read_previous(path: &Path) -> Result<Vec<Item>> {
    let conn = Connection::open_with_flags(path, OpenFlags::SQLITE_OPEN_READ_ONLY)
        .with_context(|| format!("opening {}", path.display()))?;
    let format: String = conn
        .query_row("SELECT value FROM meta WHERE key = 'format'", [], |r| {
            r.get(0)
        })
        .with_context(|| format!("{}: not an extended dictionary", path.display()))?;
    if format != FORMAT {
        bail!("{}: not an extended dictionary ({format})", path.display());
    }
    read_items(&conn)
}

/// For each old item: unchanged (same id and fingerprint), found in the same entry (by
/// entry key, else source entry number) in a group of the same kind, found once anywhere,
/// or removed.
pub fn renames(old: &[Item], new: &[Item]) -> (Vec<Rename>, RenameCounts) {
    let by_id: HashMap<String, &Item> = new.iter().map(|i| (i.id(), i)).collect();
    let mut by_entry: HashMap<&str, Vec<&Item>> = HashMap::new();
    let mut key_of_source: HashMap<i64, &str> = HashMap::new();
    let mut by_fingerprint: HashMap<(&str, bool), Vec<&Item>> = HashMap::new();
    for i in new {
        by_entry.entry(&i.entry_key).or_default().push(i);
        key_of_source.entry(i.source_entry).or_insert(&i.entry_key);
        by_fingerprint
            .entry((&i.fingerprint, i.sense.is_some()))
            .or_default()
            .push(i);
    }
    let mut out = Vec::new();
    let mut counts = RenameCounts::default();
    for o in old {
        let id = o.id();
        if by_id
            .get(&id)
            .is_some_and(|n| n.fingerprint == o.fingerprint)
        {
            counts.unchanged += 1;
            continue;
        }
        let entry = if by_entry.contains_key(o.entry_key.as_str()) {
            Some(o.entry_key.as_str())
        } else {
            key_of_source.get(&o.source_entry).copied()
        };
        let same_entry = entry.and_then(|key| {
            let found: Vec<&&Item> = by_entry[key]
                .iter()
                .filter(|n| n.kind() == o.kind() && n.fingerprint == o.fingerprint)
                .collect();
            found
                .iter()
                .find(|n| n.grp == o.grp)
                .or(found.first())
                .map(|n| n.id())
        });
        if let Some(new_id) = same_entry {
            counts.same_entry += 1;
            out.push((id, Some(new_id), "same_entry"));
            continue;
        }
        match by_fingerprint
            .get(&(o.fingerprint.as_str(), o.sense.is_some()))
            .map(Vec::as_slice)
        {
            Some([only]) => {
                counts.cross_entry += 1;
                out.push((id, Some(only.id()), "cross_entry"));
            }
            _ => {
                counts.removed += 1;
                out.push((id, None, "removed"));
            }
        }
    }
    (out, counts)
}

pub fn write(conn: &Connection, renames: &[Rename]) -> Result<()> {
    let mut stmt =
        conn.prepare("INSERT INTO sense_renames (old_id, new_id, how) VALUES (?1, ?2, ?3)")?;
    for (old, new, how) in renames {
        stmt.execute(params![old, new, how])?;
    }
    Ok(())
}
