//! Maps the sense and group ids of a previous conversion to the new ones
//! (`docs/specs/m1.5-oxford.md` 5.4), by fingerprint.

use std::collections::{HashMap, HashSet};
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
    pub changed: u64,
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
/// changed (its id is still there with another fingerprint, and no other old item moved
/// to that id: a reworded definition keeps its id), or removed.
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
    let mut found: Vec<(String, Option<(String, &'static str)>)> = Vec::new();
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
        let target = match same_entry {
            Some(new_id) => Some((new_id, "same_entry")),
            None => match by_fingerprint
                .get(&(o.fingerprint.as_str(), o.sense.is_some()))
                .map(Vec::as_slice)
            {
                Some([only]) => Some((only.id(), "cross_entry")),
                _ => None,
            },
        };
        found.push((id, target));
    }
    // Ids another old item moved to: the old item that had the id is gone (a sense
    // deleted before it), not changed.
    let moved_to: HashSet<String> = found
        .iter()
        .filter_map(|(_, t)| t.as_ref().map(|(new_id, _)| new_id.clone()))
        .collect();
    let mut out = Vec::new();
    for (id, target) in found {
        let (new_id, how) = match target {
            Some((new_id, how)) => (Some(new_id), how),
            None if by_id.contains_key(&id) && !moved_to.contains(&id) => {
                (Some(id.clone()), "changed")
            }
            None => (None, "removed"),
        };
        match how {
            "same_entry" => counts.same_entry += 1,
            "cross_entry" => counts.cross_entry += 1,
            "changed" => counts.changed += 1,
            _ => counts.removed += 1,
        }
        out.push((id, new_id, how));
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

#[cfg(test)]
mod tests {
    use super::*;

    /// Senses of one main group, numbered from 1, with these fingerprints.
    fn senses(fingerprints: &[&str]) -> Vec<Item> {
        fingerprints
            .iter()
            .enumerate()
            .map(|(n, f)| Item {
                entry_key: "blorf||verb".to_owned(),
                source_entry: 1,
                grp: "s".to_owned(),
                sense: Some(n as i64 + 1),
                fingerprint: (*f).to_owned(),
            })
            .collect()
    }

    fn table(old: &[&str], new: &[&str]) -> Vec<String> {
        renames(&senses(old), &senses(new))
            .0
            .into_iter()
            .map(|(o, n, how)| format!("{o} {} {how}", n.as_deref().unwrap_or("-")))
            .collect()
    }

    #[test]
    fn a_reworded_definition_keeps_its_id() {
        let (renames, counts) = renames(&senses(&["a", "b", "c"]), &senses(&["a", "B", "c"]));
        assert_eq!(
            renames,
            [(
                "oalecd10/blorf||verb/s/2".to_owned(),
                Some("oalecd10/blorf||verb/s/2".to_owned()),
                "changed"
            )]
        );
        assert_eq!(
            (counts.unchanged, counts.changed, counts.removed),
            (2, 1, 0)
        );
    }

    #[test]
    fn a_sense_inserted_before_moves_the_later_ones() {
        assert_eq!(
            table(&["a", "b"], &["new", "a", "b"]),
            [
                "oalecd10/blorf||verb/s/1 oalecd10/blorf||verb/s/2 same_entry",
                "oalecd10/blorf||verb/s/2 oalecd10/blorf||verb/s/3 same_entry",
            ]
        );
    }

    #[test]
    fn a_deleted_sense_is_removed_even_when_its_number_is_reused() {
        assert_eq!(
            table(&["a", "b", "c"], &["b", "c"]),
            [
                "oalecd10/blorf||verb/s/1 - removed",
                "oalecd10/blorf||verb/s/2 oalecd10/blorf||verb/s/1 same_entry",
                "oalecd10/blorf||verb/s/3 oalecd10/blorf||verb/s/2 same_entry",
            ]
        );
        // The last number disappears.
        assert_eq!(
            table(&["a", "b", "c"], &["a", "b"]),
            ["oalecd10/blorf||verb/s/3 - removed"]
        );
    }
}
