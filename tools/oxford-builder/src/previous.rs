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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum How {
    Unchanged,
    SameEntry,
    CrossEntry,
    Changed,
}

/// For each old item: unchanged (same id and fingerprint), found in the same entry (by
/// entry key, else source entry number) in a group of the same kind, found once anywhere,
/// changed (paired in order with a new item between anchors, [`pair_between_anchors`]), or
/// removed.
pub fn renames(old: &[Item], new: &[Item]) -> (Vec<Rename>, RenameCounts) {
    let by_id: HashMap<String, usize> = new.iter().enumerate().map(|(j, i)| (i.id(), j)).collect();
    let mut by_entry: HashMap<&str, Vec<usize>> = HashMap::new();
    let mut key_of_source: HashMap<i64, &str> = HashMap::new();
    let mut by_fingerprint: HashMap<(&str, bool), Vec<usize>> = HashMap::new();
    for (j, i) in new.iter().enumerate() {
        by_entry.entry(&i.entry_key).or_default().push(j);
        key_of_source.entry(i.source_entry).or_insert(&i.entry_key);
        by_fingerprint
            .entry((&i.fingerprint, i.sense.is_some()))
            .or_default()
            .push(j);
    }
    // The new entry an old item's entry became.
    let target_entry: Vec<Option<&str>> = old
        .iter()
        .map(|o| {
            if by_entry.contains_key(o.entry_key.as_str()) {
                Some(o.entry_key.as_str())
            } else {
                key_of_source.get(&o.source_entry).copied()
            }
        })
        .collect();
    let mut found: Vec<Option<(usize, How)>> = Vec::with_capacity(old.len());
    for (i, o) in old.iter().enumerate() {
        if let Some(&j) = by_id.get(&o.id())
            && new[j].fingerprint == o.fingerprint
        {
            found.push(Some((j, How::Unchanged)));
            continue;
        }
        let same_entry = target_entry[i].and_then(|key| {
            let matching: Vec<usize> = by_entry[key]
                .iter()
                .copied()
                .filter(|&j| new[j].kind() == o.kind() && new[j].fingerprint == o.fingerprint)
                .collect();
            matching
                .iter()
                .copied()
                .find(|&j| new[j].grp == o.grp)
                .or(matching.first().copied())
        });
        found.push(match same_entry {
            Some(j) => Some((j, How::SameEntry)),
            None => match by_fingerprint
                .get(&(o.fingerprint.as_str(), o.sense.is_some()))
                .map(Vec::as_slice)
            {
                Some([only]) => Some((*only, How::CrossEntry)),
                _ => None,
            },
        });
    }

    // Groups first (an entry's idioms, phrasal verbs, derivatives, each kind in order), then
    // the senses of each group, in the group it became.
    let group_number = |grp: &str| grp[1..].parse::<u32>().unwrap_or(0);
    let mut old_groups: HashMap<(&str, char), Vec<usize>> = HashMap::new();
    let mut old_senses: HashMap<(&str, &str), Vec<usize>> = HashMap::new();
    for (i, o) in old.iter().enumerate() {
        match o.sense {
            None => old_groups
                .entry((&o.entry_key, o.kind().0))
                .or_default()
                .push(i),
            Some(_) => old_senses
                .entry((&o.entry_key, &o.grp))
                .or_default()
                .push(i),
        }
    }
    let mut new_groups: HashMap<(&str, char), Vec<usize>> = HashMap::new();
    let mut new_senses: HashMap<(&str, &str), Vec<usize>> = HashMap::new();
    for (j, n) in new.iter().enumerate() {
        match n.sense {
            None => new_groups
                .entry((&n.entry_key, n.kind().0))
                .or_default()
                .push(j),
            Some(_) => new_senses
                .entry((&n.entry_key, &n.grp))
                .or_default()
                .push(j),
        }
    }
    // New items some old item was found as or paired with.
    let mut taken: HashSet<usize> = found.iter().flatten().map(|(j, _)| *j).collect();
    let mut keys: Vec<(&str, char)> = old_groups.keys().copied().collect();
    keys.sort();
    for key in keys {
        let mut olds = old_groups[&key].clone();
        olds.sort_by_key(|&i| group_number(&old[i].grp));
        let Some(entry) = target_entry[olds[0]] else {
            continue;
        };
        let mut news = new_groups.get(&(entry, key.1)).cloned().unwrap_or_default();
        news.sort_by_key(|&j| group_number(&new[j].grp));
        pair_between_anchors(&olds, &news, &mut found, &mut taken);
    }
    // The group each old group became in its new entry (the main group stays `s`).
    let mut group_of: HashMap<(&str, &str), &str> = HashMap::new();
    for (i, o) in old.iter().enumerate() {
        if o.sense.is_none()
            && let Some((j, how)) = found[i]
            && how != How::CrossEntry
        {
            group_of.insert((&o.entry_key, &o.grp), &new[j].grp);
        }
    }
    let old_group_ids: HashSet<(&str, &str)> = old
        .iter()
        .filter(|o| o.sense.is_none())
        .map(|o| (o.entry_key.as_str(), o.grp.as_str()))
        .collect();
    let mut keys: Vec<(&str, &str)> = old_senses.keys().copied().collect();
    keys.sort();
    for key in keys {
        let mut olds = old_senses[&key].clone();
        olds.sort_by_key(|&i| old[i].sense);
        let Some(entry) = target_entry[olds[0]] else {
            continue;
        };
        let has_group_item = old_group_ids.contains(&key);
        let grp = match group_of.get(&key) {
            Some(g) => *g,
            None if !has_group_item => key.1,
            // The group itself was removed or moved to another entry.
            None => continue,
        };
        let mut news = new_senses.get(&(entry, grp)).cloned().unwrap_or_default();
        news.sort_by_key(|&j| new[j].sense);
        pair_between_anchors(&olds, &news, &mut found, &mut taken);
    }

    let mut out = Vec::new();
    let mut counts = RenameCounts::default();
    for (o, f) in old.iter().zip(found) {
        let (new_id, how) = match f {
            Some((_, How::Unchanged)) => {
                counts.unchanged += 1;
                continue;
            }
            Some((j, How::SameEntry)) => {
                counts.same_entry += 1;
                (Some(new[j].id()), "same_entry")
            }
            Some((j, How::CrossEntry)) => {
                counts.cross_entry += 1;
                (Some(new[j].id()), "cross_entry")
            }
            Some((j, How::Changed)) => {
                counts.changed += 1;
                (Some(new[j].id()), "changed")
            }
            None => {
                counts.removed += 1;
                (None, "removed")
            }
        };
        out.push((o.id(), new_id, how));
    }
    (out, counts)
}

/// Pairs the old items of one sequence (`olds`: the senses of a group, or the groups of
/// one kind in an entry, in order) that were not found with the new items of the sequence
/// they became (`news`, in order) that no old item took. Anchors are the old items that are
/// unchanged or were found in `news`; when their new positions do not increase (the order
/// changed), nothing is paired. Between two anchors (and before the first, after the last),
/// the old and new items left over are paired in order as changed when there are as many
/// of each; otherwise they stay removed.
fn pair_between_anchors(
    olds: &[usize],
    news: &[usize],
    found: &mut [Option<(usize, How)>],
    taken: &mut HashSet<usize>,
) {
    if olds.iter().all(|&i| found[i].is_some()) {
        return;
    }
    let positions: HashMap<usize, usize> = news.iter().enumerate().map(|(b, &j)| (j, b)).collect();
    let position = |j: usize| positions.get(&j).copied();
    // (position in olds, position in news)
    let anchors: Vec<(usize, usize)> = olds
        .iter()
        .enumerate()
        .filter_map(|(a, &i)| match found[i] {
            Some((j, How::Unchanged | How::SameEntry)) => position(j).map(|b| (a, b)),
            _ => None,
        })
        .collect();
    if anchors.windows(2).any(|w| w[1].1 <= w[0].1) {
        return;
    }
    let mut bounds = vec![(None, None)];
    bounds.extend(anchors.iter().map(|&(a, b)| (Some(a), Some(b))));
    bounds.push((None, None));
    for w in bounds.windows(2) {
        let (start_old, start_new) = (w[0].0.map_or(0, |a| a + 1), w[0].1.map_or(0, |b| b + 1));
        let (end_old, end_new) = (w[1].0.unwrap_or(olds.len()), w[1].1.unwrap_or(news.len()));
        let left_old: Vec<usize> = olds[start_old..end_old]
            .iter()
            .copied()
            .filter(|&i| found[i].is_none())
            .collect();
        let left_new: Vec<usize> = news[start_new..end_new]
            .iter()
            .copied()
            .filter(|j| !taken.contains(j))
            .collect();
        if !left_old.is_empty() && left_old.len() == left_new.len() {
            for (i, j) in left_old.into_iter().zip(left_new) {
                found[i] = Some((j, How::Changed));
                taken.insert(j);
            }
        }
    }
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
            .map(|(o, n, how)| {
                let short = |id: &str| id.trim_start_matches("oalecd10/blorf||verb/").to_owned();
                format!(
                    "{} {} {how}",
                    short(&o),
                    n.as_deref().map_or("-".to_owned(), short)
                )
            })
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
            ["s/1 s/2 same_entry", "s/2 s/3 same_entry",]
        );
    }

    #[test]
    fn a_deleted_sense_is_removed_even_when_its_number_is_reused() {
        assert_eq!(
            table(&["a", "b", "c"], &["b", "c"]),
            ["s/1 - removed", "s/2 s/1 same_entry", "s/3 s/2 same_entry",]
        );
        // The last number disappears.
        assert_eq!(table(&["a", "b", "c"], &["a", "b"]), ["s/3 - removed"]);
    }

    #[test]
    fn deleted_or_inserted_and_reworded_in_one_group() {
        // The first deleted, the third reworded.
        assert_eq!(
            table(&["a", "b", "c"], &["b", "C"]),
            ["s/1 - removed", "s/2 s/1 same_entry", "s/3 s/2 changed"]
        );
        // One inserted before, the second reworded.
        assert_eq!(
            table(&["a", "b"], &["n", "a", "B"]),
            ["s/1 s/2 same_entry", "s/2 s/3 changed"]
        );
        // One deleted and one reworded: which is which cannot be told.
        assert_eq!(
            table(&["a", "b", "c"], &["B", "c"]),
            ["s/1 - removed", "s/2 - removed", "s/3 s/2 same_entry"]
        );
    }

    #[test]
    fn nothing_is_paired_when_the_order_changed() {
        assert_eq!(
            table(&["a", "b", "c"], &["c", "B", "a"]),
            ["s/1 s/3 same_entry", "s/2 - removed", "s/3 s/1 same_entry"]
        );
    }

    #[test]
    fn groups_are_paired_like_senses() {
        let group = |n: u32, f: &str| Item {
            entry_key: "blorf||verb".to_owned(),
            source_entry: 1,
            grp: format!("i{n}"),
            sense: None,
            fingerprint: f.to_owned(),
        };
        let old = [group(1, "x"), group(2, "y")];
        let new = [group(1, "n"), group(2, "x"), group(3, "Y")];
        let (renames, _) = renames(&old, &new);
        assert_eq!(
            renames,
            [
                (
                    "oalecd10/blorf||verb/i1".to_owned(),
                    Some("oalecd10/blorf||verb/i2".to_owned()),
                    "same_entry"
                ),
                (
                    "oalecd10/blorf||verb/i2".to_owned(),
                    Some("oalecd10/blorf||verb/i3".to_owned()),
                    "changed"
                ),
            ]
        );
    }
}
