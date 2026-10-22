//! Writes the extended dictionary's tables (`docs/specs/m1.5-oxford.md` 4) into a new
//! database. Rows go in a fixed order so that the same input gives the same file.

use std::collections::{BTreeMap, HashMap};

use anyhow::Result;
use lextrail_dict::ext::id::{DupCounter, Group};
use lextrail_dict::ext::schema::{INDEXES, RELATIONS, SCHEMA};
use lextrail_dict::ext::text::fold_for_match;
use lextrail_dict::normalize_query;
use rusqlite::{Connection, params};

use crate::ecdict::Ecdict;
use crate::relation::{Relation, note_base_fits, note_targets, relate};
use crate::source::SourceEntry;

/// An entry ready to write: its identity key and the relation of each lookup key.
pub struct Prepared<'a> {
    pub id: i64,
    pub item: &'a SourceEntry,
    /// `EntryKey` text (D052).
    pub entry_key: String,
    pub ord: i64,
    pub keys: Vec<(String, Relation)>,
}

#[derive(Debug, Default, Clone, serde::Serialize)]
pub struct Counts {
    pub entries: u64,
    pub duplicate_entry_keys: u64,
    pub keys: u64,
    pub keys_dropped_empty: u64,
    pub relations: BTreeMap<String, u64>,
    pub groups: BTreeMap<String, u64>,
    pub senses: u64,
    pub senses_alt_fingerprint: u64,
    /// Senses whose fingerprint repeats another sense's in the same group.
    pub senses_duplicate_fingerprint_in_group: u64,
    pub examples: u64,
    pub forms: u64,
    pub variants: u64,
    pub notes: u64,
    pub with_phon_gb: u64,
    pub with_phon_us: u64,
    pub with_cefr: u64,
    pub oxford_3000: u64,
    pub oxford_5000: u64,
    pub with_opal: u64,
}

fn homograph_number(h: &str) -> u32 {
    h.parse().unwrap_or(0)
}

/// Gives each entry its identity key, order among same-headword entries and key relations.
pub fn prepare<'a>(entries: &'a [SourceEntry], ecdict: &Ecdict) -> Vec<Prepared<'a>> {
    let mut dups = DupCounter::default();
    let mut prepared: Vec<Prepared> = entries
        .iter()
        .enumerate()
        .map(|(i, item)| {
            let e = &item.entry;
            let key = dups.key(&e.headword, &e.homograph, &e.pos);
            Prepared {
                id: i as i64 + 1,
                item,
                entry_key: key.to_string(),
                ord: 0,
                keys: item
                    .keys
                    .iter()
                    .map(|k| (k.clone(), relate(k, e, ecdict)))
                    .collect(),
            }
        })
        .collect();
    add_note_relations(&mut prepared);
    // Same headword: by homograph number, then source order (6.1).
    let mut by_headword: HashMap<String, Vec<usize>> = HashMap::new();
    for (i, p) in prepared.iter().enumerate() {
        by_headword
            .entry(p.item.entry.headword.to_lowercase())
            .or_default()
            .push(i);
    }
    for group in by_headword.values_mut() {
        group.sort_by_key(|&i| {
            let e = &prepared[i].item.entry;
            (homograph_number(&e.homograph), e.source_entry)
        });
        for (ord, &i) in group.iter().enumerate() {
            prepared[i].ord = ord as i64;
        }
    }
    prepared
}

/// A form note (`sped`: "past tense, past participle of speed") whose key the converter
/// could not relate to its base form gets that relation from its cross-reference: the key
/// becomes an inflection of the target entry (`docs/specs/m1.5-oxford.md` 6.1). A row the
/// key already has with that entry as `other` or part of an idiom is replaced; otherwise a
/// row is added, so `keys` has rows that are not lookup keys of that entry in the MDX.
fn add_note_relations(prepared: &mut [Prepared]) {
    let mut by_headword: HashMap<String, Vec<usize>> = HashMap::new();
    for (i, p) in prepared.iter().enumerate() {
        by_headword
            .entry(fold_for_match(&p.item.entry.headword))
            .or_default()
            .push(i);
    }
    // (base entry, key, relation)
    let mut wanted: Vec<(usize, String, Relation)> = Vec::new();
    for (i, p) in prepared.iter().enumerate() {
        let Some(targets) = note_targets(&p.item.entry) else {
            continue;
        };
        let keys = p.keys.iter().filter(|(_, r)| r.relation == "headword");
        for (key, _) in keys {
            for t in &targets {
                let bases = by_headword
                    .get(&fold_for_match(&t.headword))
                    .into_iter()
                    .flatten()
                    .copied()
                    .filter(|&j| j != i && note_base_fits(t, &prepared[j].item.entry));
                for j in bases {
                    wanted.push((
                        j,
                        key.clone(),
                        Relation {
                            relation: "inflection",
                            kinds: t.kinds.clone(),
                            via: Some(prepared[j].item.entry.headword.clone()),
                            group: None,
                        },
                    ));
                }
            }
        }
    }
    for (j, key, relation) in wanted {
        let keys = &mut prepared[j].keys;
        match keys.iter_mut().find(|(k, _)| *k == key) {
            Some((_, r)) if matches!(r.relation, "other" | "idiom_part" | "phrasal_part") => {
                *r = relation;
            }
            Some(_) => {}
            None => keys.push((key, relation)),
        }
    }
}

fn group_text(g: Option<Group>) -> Option<String> {
    g.map(|g| g.to_string())
}

fn json<T: serde::Serialize>(items: &[T]) -> Result<Option<String>> {
    Ok(if items.is_empty() {
        None
    } else {
        Some(serde_json::to_string(items)?)
    })
}

/// Writes all tables except `meta` and `sense_renames` into `conn` (a new, empty database).
pub fn write(conn: &mut Connection, prepared: &[Prepared], ecdict: &Ecdict) -> Result<Counts> {
    conn.execute_batch(
        "PRAGMA page_size = 4096; PRAGMA journal_mode = OFF; PRAGMA synchronous = OFF;",
    )?;
    conn.execute_batch(SCHEMA)?;
    let mut c = Counts::default();
    let tx = conn.transaction()?;
    {
        let mut entry_stmt = tx.prepare(
            "INSERT INTO entries (id, source_entry, entry_key, headword, homograph, pos, phon_gb,
             phon_us, sound_gb, sound_us, cefr, oxford, opal, ord)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14)",
        )?;
        let mut group_stmt = tx.prepare(
            "INSERT INTO groups (entry_id, grp, ord, label, pos, xrefs, fingerprint)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
        )?;
        let mut sense_stmt = tx.prepare(
            "INSERT INTO senses (entry_id, grp, sense, cefr, gram, labels, hint_en, hint_zh,
             heading_en, heading_zh, patterns, def_en, def_zh, xrefs, fingerprint)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15)",
        )?;
        let mut example_stmt = tx.prepare(
            "INSERT INTO examples (entry_id, grp, sense, ord, pattern, en, zh, sound_gb, sound_us)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
        )?;
        let mut form_stmt = tx.prepare(
            "INSERT INTO forms (entry_id, ord, grp, kind, text, sound_gb, sound_us)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
        )?;
        let mut variant_stmt = tx.prepare(
            "INSERT INTO variants (entry_id, ord, grp, label, text) VALUES (?1, ?2, ?3, ?4, ?5)",
        )?;
        let mut note_stmt = tx.prepare(
            "INSERT INTO notes (entry_id, ord, grp, kind, text) VALUES (?1, ?2, ?3, ?4, ?5)",
        )?;
        for p in prepared {
            let e = &p.item.entry;
            c.entries += 1;
            if p.entry_key.matches('|').count() > 2 {
                c.duplicate_entry_keys += 1;
            }
            c.with_phon_gb += e.phon_gb.is_some() as u64;
            c.with_phon_us += e.phon_us.is_some() as u64;
            c.with_cefr += e.cefr.is_some() as u64;
            c.oxford_3000 += (e.oxford == Some(3000)) as u64;
            c.oxford_5000 += (e.oxford == Some(5000)) as u64;
            c.with_opal += e.opal.is_some() as u64;
            entry_stmt.execute(params![
                p.id,
                e.source_entry,
                p.entry_key,
                e.headword,
                Some(&e.homograph).filter(|h| !h.is_empty()),
                Some(e.pos.join(", ")).filter(|s| !s.is_empty()),
                e.phon_gb,
                e.phon_us,
                e.sound_gb,
                e.sound_us,
                e.cefr,
                e.oxford,
                e.opal,
                p.ord,
            ])?;
            for (ord, g) in e.groups.iter().enumerate() {
                let grp = g.group.to_string();
                let kind = match g.group {
                    Group::Main => "main",
                    Group::Idiom(_) => "idiom",
                    Group::PhrasalVerb(_) => "phrasal",
                    Group::Derivative(_) => "derivative",
                };
                *c.groups.entry(kind.to_owned()).or_default() += 1;
                group_stmt.execute(params![
                    p.id,
                    grp,
                    ord as i64,
                    g.label,
                    Some(g.pos.join(", ")).filter(|s| !s.is_empty()),
                    json(&g.xrefs)?,
                    g.fingerprint(),
                ])?;
                let mut seen = std::collections::HashSet::new();
                for s in &g.senses {
                    c.senses += 1;
                    c.senses_alt_fingerprint += s.fingerprint.starts_with("alt:") as u64;
                    if !seen.insert(&s.fingerprint) {
                        c.senses_duplicate_fingerprint_in_group += 1;
                    }
                    sense_stmt.execute(params![
                        p.id,
                        grp,
                        s.n,
                        s.cefr,
                        s.gram,
                        s.labels,
                        s.hint_en,
                        s.hint_zh,
                        s.heading_en,
                        s.heading_zh,
                        json(&s.patterns)?,
                        s.def_en,
                        s.def_zh,
                        json(&s.xrefs)?,
                        s.fingerprint,
                    ])?;
                    for (ord, x) in s.examples.iter().enumerate() {
                        c.examples += 1;
                        example_stmt.execute(params![
                            p.id,
                            grp,
                            s.n,
                            ord as i64 + 1,
                            x.pattern,
                            x.en,
                            x.zh,
                            x.sound_gb,
                            x.sound_us,
                        ])?;
                    }
                }
            }
            for (ord, f) in e.forms.iter().enumerate() {
                c.forms += 1;
                form_stmt.execute(params![
                    p.id,
                    ord as i64 + 1,
                    group_text(f.group),
                    f.kind,
                    f.text,
                    f.sound_gb,
                    f.sound_us,
                ])?;
            }
            for (ord, v) in e.variants.iter().enumerate() {
                c.variants += 1;
                variant_stmt.execute(params![
                    p.id,
                    ord as i64 + 1,
                    group_text(v.group),
                    v.label,
                    v.text
                ])?;
            }
            for (ord, n) in e.notes.iter().enumerate() {
                c.notes += 1;
                note_stmt.execute(params![
                    p.id,
                    ord as i64 + 1,
                    group_text(n.group),
                    n.kind,
                    n.text
                ])?;
            }
        }

        write_keys(&tx, prepared, ecdict, &mut c)?;
    }
    tx.commit()?;
    conn.execute_batch(INDEXES)?;
    Ok(c)
}

/// `keys`, with `ord` among the rows of the same key (case-insensitive): by relation, then
/// the entry headword's ECDICT frequency, homograph number and source order (6.1, 6.3).
fn write_keys(
    tx: &rusqlite::Transaction,
    prepared: &[Prepared],
    ecdict: &Ecdict,
    c: &mut Counts,
) -> Result<()> {
    struct Row<'a> {
        key: String,
        key_exact: &'a str,
        p: &'a Prepared<'a>,
        rel: &'a Relation,
    }
    let mut rows: Vec<Row> = Vec::new();
    for p in prepared {
        for (key_exact, rel) in &p.keys {
            let key = normalize_query(key_exact);
            if key.is_empty() {
                c.keys_dropped_empty += 1;
                continue;
            }
            rows.push(Row {
                key,
                key_exact,
                p,
                rel,
            });
        }
    }
    let mut by_key: BTreeMap<String, Vec<usize>> = BTreeMap::new();
    for (i, r) in rows.iter().enumerate() {
        by_key.entry(r.key.to_lowercase()).or_default().push(i);
    }
    let mut ord = vec![0i64; rows.len()];
    for group in by_key.values_mut() {
        group.sort_by_key(|&i| {
            let r = &rows[i];
            let e = &r.p.item.entry;
            (
                RELATIONS.iter().position(|x| *x == r.rel.relation),
                ecdict.frequency(&e.headword).unwrap_or(i64::MAX),
                homograph_number(&e.homograph),
                e.source_entry,
                r.key_exact,
            )
        });
        for (n, &i) in group.iter().enumerate() {
            ord[i] = n as i64;
        }
    }
    let mut stmt = tx.prepare(
        "INSERT INTO keys (key, key_exact, entry_id, relation, kinds, via, grp, ord)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
    )?;
    for (i, r) in rows.iter().enumerate() {
        c.keys += 1;
        *c.relations.entry(r.rel.relation.to_owned()).or_default() += 1;
        stmt.execute(params![
            r.key,
            r.key_exact,
            r.p.id,
            r.rel.relation,
            Some(r.rel.kinds.join(" ")).filter(|k| !k.is_empty()),
            r.rel.via,
            group_text(r.rel.group),
            ord[i],
        ])?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::parse::{Entry, Form, GroupData, SenseData, Xref};

    fn entry(headword: &str, homograph: &str, pos: &[&str], senses: Vec<SenseData>) -> Entry {
        Entry {
            headword: headword.into(),
            homograph: homograph.into(),
            pos: pos.iter().map(|p| p.to_string()).collect(),
            groups: vec![GroupData {
                group: Group::Main,
                label: None,
                label_parts: Vec::new(),
                pos: Vec::new(),
                xrefs: Vec::new(),
                senses,
            }],
            ..Entry::default()
        }
    }

    fn defined() -> Vec<SenseData> {
        vec![SenseData {
            n: 1,
            def_en: Some("to make a low sound".into()),
            ..SenseData::default()
        }]
    }

    fn note(kind: &str, target: &str) -> Vec<SenseData> {
        vec![SenseData {
            n: 1,
            xrefs: vec![Xref {
                kind: kind.into(),
                label: "past tense of".into(),
                targets: vec![target.into()],
            }],
            ..SenseData::default()
        }]
    }

    fn form(kind: &str, text: &str) -> Form {
        Form {
            group: None,
            kind: Some(kind.into()),
            text: text.into(),
            verb_form: true,
            sound_gb: None,
            sound_us: None,
        }
    }

    fn rows(prepared: &[Prepared], key: &str) -> Vec<String> {
        prepared
            .iter()
            .flat_map(|p| {
                p.keys.iter().filter(|(k, _)| k == key).map(move |(_, r)| {
                    format!(
                        "{}{} {} {}",
                        p.item.entry.headword,
                        p.item.entry.homograph,
                        r.relation,
                        r.kinds.join(" ")
                    )
                })
            })
            .collect()
    }

    #[test]
    fn form_notes_relate_their_keys_to_the_target() {
        let mut snib1 = entry("snib", "1", &["verb"], defined());
        snib1.forms = vec![form("root", "snib"), form("past", "snibbed")];
        let snib2 = entry("snib", "2", &["verb"], defined());
        let snab = entry("snab", "", &["noun"], defined());
        let entries = [
            (snib1, vec!["snib"]),
            (snib2, vec!["snib", "snub"]),
            (snab, vec!["snab"]),
            // "past tense of snib2": not snib1.
            (entry("snub", "", &[], note("ptof", "snib2")), vec!["snub"]),
            // A plural needs a noun: snab, not the verbs.
            (entry("snaba", "", &[], note("plof", "snab")), vec!["snaba"]),
            // The target's table need not list the form; "(2)" is a sense number.
            (
                entry("snob", "", &[], note("ptof", "snib (2)")),
                vec!["snob"],
            ),
        ];
        let source: Vec<SourceEntry> = entries
            .into_iter()
            .enumerate()
            .map(|(i, (mut entry, keys))| {
                entry.source_entry = i as u32 + 1;
                SourceEntry {
                    entry,
                    keys: keys.into_iter().map(str::to_owned).collect(),
                }
            })
            .collect();
        let prepared = prepare(&source, &Ecdict::default());
        // snib2 had `snub` as a lookup key (`other`): replaced.
        assert_eq!(
            rows(&prepared, "snub"),
            ["snib2 inflection p", "snub headword "]
        );
        // snab had no `snaba` row: added.
        assert_eq!(
            rows(&prepared, "snaba"),
            ["snab inflection s", "snaba headword "]
        );
        assert_eq!(
            rows(&prepared, "snob"),
            ["snib1 inflection p", "snib2 inflection p", "snob headword "]
        );
    }
}
