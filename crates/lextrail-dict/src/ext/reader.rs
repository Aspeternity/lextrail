//! Reading an extended dictionary file (`docs/specs/m1.5-oxford.md` 4, 6.1): the rows of a
//! lookup key and whole entries. Merging with ECDICT is [`crate::merge`].

use std::path::Path;

use rusqlite::{Connection, OpenFlags, OptionalExtension};

use super::schema::{FORMAT, FORMAT_VERSION, Xref};
use super::text::fold_for_match;
use crate::DictError;

/// Same page cache as the ECDICT dictionary, no memory mapping (spec 10, D046).
const CACHE_KIB: i64 = 1024;

/// How a lookup key relates to an entry, decided by the converter (spec 6.1). In the
/// order the converter tries them.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Relation {
    Headword,
    Inflection,
    Variant,
    Derivative,
    /// The key is a whole idiom.
    Idiom,
    /// The key is a whole phrasal verb.
    Phrasal,
    /// The key is part of a longer idiom.
    IdiomPart,
    /// The key is part of a longer phrasal verb.
    PhrasalPart,
    Other,
}

impl Relation {
    fn from_name(name: &str) -> Option<Self> {
        Some(match name {
            "headword" => Self::Headword,
            "inflection" => Self::Inflection,
            "variant" => Self::Variant,
            "derivative" => Self::Derivative,
            "idiom" => Self::Idiom,
            "phrasal" => Self::Phrasal,
            "idiom_part" => Self::IdiomPart,
            "phrasal_part" => Self::PhrasalPart,
            "other" => Self::Other,
            _ => return None,
        })
    }

    /// `inflection`, `variant` or `derivative`: the key's own entry, if any, is a form of
    /// this one (spec 6.3 `FormOf`).
    pub fn is_form(self) -> bool {
        matches!(self, Self::Inflection | Self::Variant | Self::Derivative)
    }
}

/// A row of `keys`: one entry a lookup key leads to.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct KeyRow {
    /// The key as the source wrote it (`keys.key_exact`).
    pub key: String,
    pub entry_id: i64,
    pub relation: Relation,
    /// Kinds of form for an inflection (`p`, `d`, `3`, `s`, `neg` ...).
    pub kinds: Vec<String>,
    /// The form the relation goes through: the base form of an inflection, the variant,
    /// the derivative, idiom or phrasal verb.
    pub via: Option<String>,
    /// The idiom, phrasal verb or derivative group the key leads to (`p27`).
    pub group: Option<String>,
}

/// An entry with everything the popup can show.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExtEntry {
    pub id: i64,
    /// `oalecd10/<entry key>`: the prefix of its group and sense ids.
    pub id_prefix: String,
    pub headword: String,
    pub homograph: Option<String>,
    pub pos: Vec<String>,
    pub phon_gb: Option<String>,
    pub phon_us: Option<String>,
    pub sound_gb: Option<String>,
    pub sound_us: Option<String>,
    /// Entry-level CEFR level (`B2`).
    pub cefr: Option<String>,
    /// 3000 or 5000 for the Oxford 3000 / 5000.
    pub oxford: Option<u32>,
    /// Academic word list (OPAL): `W`, `S`.
    pub opal: Option<String>,
    /// Main senses first, then idioms, phrasal verbs and derivatives in source order.
    pub groups: Vec<ExtGroup>,
    pub forms: Vec<ExtForm>,
    pub variants: Vec<ExtVariant>,
}

impl ExtEntry {
    /// Has an entry-level CEFR level, an Oxford 3000 / 5000 or OPAL mark (spec 6.3).
    pub fn is_marked(&self) -> bool {
        self.cefr.is_some() || self.oxford.is_some() || self.opal.is_some()
    }

    /// Only says which word it is a form of (`went`: "past tense of go"): every sense has
    /// no definition and only such cross-references. Like ECDICT's "xx 的过去式" entries,
    /// it is not shown when its base form is (spec 6.3).
    pub fn is_form_note(&self) -> bool {
        let senses: Vec<&ExtSense> = self.groups.iter().flat_map(|g| &g.senses).collect();
        !senses.is_empty()
            && senses.iter().all(|s| {
                s.def_en.is_none()
                    && s.def_zh.is_none()
                    && !s.xrefs.is_empty()
                    && s.xrefs.iter().all(Xref::is_form_of)
            })
    }

    /// Whether this form note (`went`: "past tense of go") points to the entry `headword`
    /// (targets may carry a homograph number: `lie1`).
    pub fn is_form_note_of(&self, headword: &str) -> bool {
        self.groups
            .iter()
            .flat_map(|g| &g.senses)
            .flat_map(|s| &s.xrefs)
            .filter(|x| x.is_form_of())
            .flat_map(|x| &x.targets)
            .any(|t| names_entry(t, headword))
    }

    /// Has a sense or group without a definition, only cross-references, one of which
    /// points to `headword` (`gaol`: "= jail"; `program`: "(NAmE) = programme noun").
    pub fn cross_refers_to(&self, headword: &str) -> bool {
        let refers = |xrefs: &[Xref]| {
            xrefs
                .iter()
                .flat_map(|x| &x.targets)
                .any(|t| names_entry(t, headword))
        };
        self.groups.iter().any(|g| {
            (g.senses.is_empty() && refers(&g.xrefs))
                || g.senses
                    .iter()
                    .any(|s| s.is_cross_reference_only() && refers(&s.xrefs))
        })
    }

    /// Some sense has a definition (English or Chinese).
    pub fn has_definitions(&self) -> bool {
        self.groups
            .iter()
            .flat_map(|g| &g.senses)
            .any(|s| s.def_en.is_some() || s.def_zh.is_some())
    }

    pub fn group(&self, grp: &str) -> Option<&ExtGroup> {
        self.groups.iter().find(|g| g.grp == grp)
    }
}

impl ExtGroup {
    /// No sense has a definition; there are only cross-references (a phrasal verb that
    /// says "= another one").
    pub fn is_cross_reference_only(&self) -> bool {
        if self.senses.is_empty() {
            return !self.xrefs.is_empty();
        }
        self.senses
            .iter()
            .all(|s| s.def_en.is_none() && s.def_zh.is_none())
            && (!self.xrefs.is_empty() || self.senses.iter().any(|s| !s.xrefs.is_empty()))
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExtGroup {
    /// `s`, `i3`, `p1`, `d2`.
    pub grp: String,
    /// Stable id (D052), e.g. `oalecd10/blorf||verb/i1`.
    pub id: String,
    /// The idiom, phrasal verb or derivative itself; `None` for the main senses.
    pub label: Option<String>,
    pub pos: Vec<String>,
    pub xrefs: Vec<Xref>,
    pub senses: Vec<ExtSense>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExtSense {
    /// The sense number in its group, from 1, as printed.
    pub n: u32,
    /// Stable id (D052), e.g. `oalecd10/blorf||verb/s/2`.
    pub id: String,
    pub cefr: Option<String>,
    pub gram: Option<String>,
    pub labels: Option<String>,
    pub hint_en: Option<String>,
    pub hint_zh: Option<String>,
    pub heading_en: Option<String>,
    pub heading_zh: Option<String>,
    pub patterns: Vec<String>,
    pub def_en: Option<String>,
    pub def_zh: Option<String>,
    pub xrefs: Vec<Xref>,
    pub examples: Vec<ExtExample>,
}

impl ExtSense {
    /// No definition, only cross-references.
    pub fn is_cross_reference_only(&self) -> bool {
        self.def_en.is_none() && self.def_zh.is_none() && !self.xrefs.is_empty()
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExtExample {
    pub pattern: Option<String>,
    pub en: String,
    pub zh: Option<String>,
    pub sound_gb: Option<String>,
    pub sound_us: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExtForm {
    pub group: Option<String>,
    pub kind: Option<String>,
    pub text: String,
    pub sound_gb: Option<String>,
    pub sound_us: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExtVariant {
    pub group: Option<String>,
    pub label: Option<String>,
    pub text: String,
}

/// An open extended dictionary file, read-only. Not `Sync`: share it behind a mutex.
#[derive(Debug)]
pub struct ExtDictionary {
    conn: Connection,
    source: String,
}

impl ExtDictionary {
    /// Opens an extended dictionary and checks its format and version.
    pub fn open(path: &Path) -> Result<Self, DictError> {
        if !path.is_file() {
            return Err(DictError::NotFound(path.to_owned()));
        }
        let conn = Connection::open_with_flags(
            path,
            OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_NO_MUTEX,
        )?;
        let meta = |key: &str| -> Option<String> {
            conn.query_row("SELECT value FROM meta WHERE key = ?1", [key], |r| r.get(0))
                .optional()
                .unwrap_or(None)
        };
        let (format, version) = (meta("format"), meta("format_version"));
        if format.as_deref() != Some(FORMAT)
            || version.as_deref() != Some(&FORMAT_VERSION.to_string())
        {
            return Err(DictError::UnsupportedFormat {
                found: Some(format!(
                    "{} {}",
                    format.unwrap_or_default(),
                    version.unwrap_or_default()
                )),
            });
        }
        let source = meta("source").unwrap_or_default();
        conn.execute_batch(&format!(
            "PRAGMA cache_size = -{CACHE_KIB}; PRAGMA mmap_size = 0;"
        ))?;
        Ok(Self { conn, source })
    }

    /// A `meta` value.
    pub(crate) fn meta(&self, key: &str) -> Result<Option<String>, DictError> {
        Ok(self
            .conn
            .query_row("SELECT value FROM meta WHERE key = ?1", [key], |r| r.get(0))
            .optional()?)
    }

    /// `meta.source`, e.g. `oalecd10`.
    pub fn source(&self) -> &str {
        &self.source
    }

    /// The rows of `key` (normalized; any case), in `keys.ord` order: by relation, ECDICT
    /// frequency of the headword, homograph number, source order (spec 6.1).
    pub fn rows(&self, key: &str) -> Result<Vec<KeyRow>, DictError> {
        let mut stmt = self.conn.prepare_cached(
            "SELECT key_exact, entry_id, relation, kinds, via, grp FROM keys
             WHERE key = ?1 ORDER BY ord, key_exact",
        )?;
        let rows = stmt
            .query_map([key], |r| {
                Ok((
                    r.get::<_, String>(0)?,
                    r.get::<_, i64>(1)?,
                    r.get::<_, String>(2)?,
                    r.get::<_, Option<String>>(3)?,
                    r.get::<_, Option<String>>(4)?,
                    r.get::<_, Option<String>>(5)?,
                ))
            })?
            .collect::<Result<Vec<_>, _>>()?;
        rows.into_iter()
            .map(|(key, entry_id, relation, kinds, via, group)| {
                Ok(KeyRow {
                    key,
                    entry_id,
                    relation: Relation::from_name(&relation)
                        .ok_or_else(|| DictError::Data(format!("relation {relation:?}")))?,
                    kinds: kinds
                        .map(|k| k.split(' ').map(str::to_owned).collect())
                        .unwrap_or_default(),
                    via,
                    group,
                })
            })
            .collect()
    }

    /// Entries for a base form found by ECDICT (spec 6.2 step 3): those with `word` as
    /// headword, else those with `word` as a whole idiom or phrasal verb (with the group).
    pub fn lemma_entries(&self, word: &str) -> Result<Vec<(i64, Option<String>)>, DictError> {
        let rows = self.rows(word)?;
        let mut out: Vec<(i64, Option<String>)> = Vec::new();
        for wanted in [
            &[Relation::Headword][..],
            &[Relation::Idiom, Relation::Phrasal][..],
        ] {
            for r in rows.iter().filter(|r| wanted.contains(&r.relation)) {
                if !out.iter().any(|(id, _)| *id == r.entry_id) {
                    out.push((r.entry_id, r.group.clone()));
                }
            }
            if !out.is_empty() {
                break;
            }
        }
        Ok(out)
    }

    /// Whether `key` (normalized; any case) is a lookup key, with any relation.
    pub fn has_key(&self, key: &str) -> Result<bool, DictError> {
        let mut stmt = self
            .conn
            .prepare_cached("SELECT 1 FROM keys WHERE key = ?1 LIMIT 1")?;
        Ok(stmt.exists([key])?)
    }

    /// The whole entry.
    pub fn entry(&self, id: i64) -> Result<ExtEntry, DictError> {
        let mut entry = self.conn.query_row(
            "SELECT entry_key, headword, homograph, pos, phon_gb, phon_us, sound_gb, sound_us,
             cefr, oxford, opal FROM entries WHERE id = ?1",
            [id],
            |r| {
                Ok(ExtEntry {
                    id,
                    id_prefix: format!("{}/{}", self.source, r.get::<_, String>(0)?),
                    headword: r.get(1)?,
                    homograph: r.get(2)?,
                    pos: list(r.get(3)?),
                    phon_gb: r.get(4)?,
                    phon_us: r.get(5)?,
                    sound_gb: r.get(6)?,
                    sound_us: r.get(7)?,
                    cefr: r.get(8)?,
                    oxford: r.get(9)?,
                    opal: r.get(10)?,
                    groups: Vec::new(),
                    forms: Vec::new(),
                    variants: Vec::new(),
                })
            },
        )?;
        let mut stmt = self.conn.prepare_cached(
            "SELECT grp, label, pos, xrefs FROM groups WHERE entry_id = ?1 ORDER BY ord",
        )?;
        let groups = stmt
            .query_map([id], |r| {
                Ok((
                    r.get::<_, String>(0)?,
                    r.get::<_, Option<String>>(1)?,
                    r.get::<_, Option<String>>(2)?,
                    r.get::<_, Option<String>>(3)?,
                ))
            })?
            .collect::<Result<Vec<_>, _>>()?;
        for (grp, label, pos, xrefs) in groups {
            entry.groups.push(ExtGroup {
                id: format!("{}/{grp}", entry.id_prefix),
                grp,
                label,
                pos: list(pos),
                xrefs: json(xrefs)?,
                senses: Vec::new(),
            });
        }
        let mut examples = self.examples(id)?;
        let mut stmt = self.conn.prepare_cached(
            "SELECT grp, sense, cefr, gram, labels, hint_en, hint_zh, heading_en, heading_zh,
             patterns, def_en, def_zh, xrefs FROM senses WHERE entry_id = ?1 ORDER BY id",
        )?;
        type SenseRow = (String, u32, [Option<String>; 11]);
        let senses: Vec<SenseRow> = stmt
            .query_map([id], |r| {
                let mut cols: [Option<String>; 11] = Default::default();
                for (i, c) in cols.iter_mut().enumerate() {
                    *c = r.get(i + 2)?;
                }
                Ok((r.get(0)?, r.get(1)?, cols))
            })?
            .collect::<Result<_, _>>()?;
        for (grp, n, cols) in senses {
            let [
                cefr,
                gram,
                labels,
                hint_en,
                hint_zh,
                heading_en,
                heading_zh,
                patterns,
                def_en,
                def_zh,
                xrefs,
            ] = cols;
            let Some(group) = entry.groups.iter_mut().find(|g| g.grp == grp) else {
                return Err(DictError::Data(format!("sense of a missing group {grp}")));
            };
            group.senses.push(ExtSense {
                n,
                id: format!("{}/{n}", group.id),
                cefr,
                gram,
                labels,
                hint_en,
                hint_zh,
                heading_en,
                heading_zh,
                patterns: json(patterns)?,
                def_en,
                def_zh,
                xrefs: json(xrefs)?,
                examples: examples.remove(&(grp.clone(), n)).unwrap_or_default(),
            });
        }
        let mut stmt = self.conn.prepare_cached(
            "SELECT grp, kind, text, sound_gb, sound_us FROM forms WHERE entry_id = ?1
             ORDER BY ord",
        )?;
        entry.forms = stmt
            .query_map([id], |r| {
                Ok(ExtForm {
                    group: r.get(0)?,
                    kind: r.get(1)?,
                    text: r.get(2)?,
                    sound_gb: r.get(3)?,
                    sound_us: r.get(4)?,
                })
            })?
            .collect::<Result<_, _>>()?;
        let mut stmt = self.conn.prepare_cached(
            "SELECT grp, label, text FROM variants WHERE entry_id = ?1 ORDER BY ord",
        )?;
        entry.variants = stmt
            .query_map([id], |r| {
                Ok(ExtVariant {
                    group: r.get(0)?,
                    label: r.get(1)?,
                    text: r.get(2)?,
                })
            })?
            .collect::<Result<_, _>>()?;
        Ok(entry)
    }

    /// Examples of an entry by (group, sense number).
    fn examples(
        &self,
        id: i64,
    ) -> Result<std::collections::HashMap<(String, u32), Vec<ExtExample>>, DictError> {
        let mut stmt = self.conn.prepare_cached(
            "SELECT grp, sense, pattern, en, zh, sound_gb, sound_us FROM examples
             WHERE entry_id = ?1 ORDER BY id",
        )?;
        let mut out: std::collections::HashMap<(String, u32), Vec<ExtExample>> =
            std::collections::HashMap::new();
        let rows = stmt.query_map([id], |r| {
            Ok((
                (r.get::<_, String>(0)?, r.get::<_, u32>(1)?),
                ExtExample {
                    pattern: r.get(2)?,
                    en: r.get(3)?,
                    zh: r.get(4)?,
                    sound_gb: r.get(5)?,
                    sound_us: r.get(6)?,
                },
            ))
        })?;
        for row in rows {
            let (at, example) = row?;
            out.entry(at).or_default().push(example);
        }
        Ok(out)
    }
}

/// Parts of speech that follow a cross-reference target (`programme noun`).
const TARGET_POS: &[&str] = &[
    "noun",
    "verb",
    "adjective",
    "adverb",
    "preposition",
    "pronoun",
    "conjunction",
    "exclamation",
    "determiner",
    "prefix",
    "suffix",
    "abbreviation",
    "number",
    "symbol",
];

/// Whether a cross-reference target names the entry `headword`: compared folded, without
/// a homograph number, a parenthesized sense number or a following part of speech.
fn names_entry(target: &str, headword: &str) -> bool {
    let mut text = String::new();
    let mut depth = 0;
    for c in target.chars() {
        match c {
            '(' => depth += 1,
            ')' if depth > 0 => depth -= 1,
            c if depth == 0 && !c.is_ascii_digit() => text.push(c),
            _ => {}
        }
    }
    let words: Vec<&str> = text.split_whitespace().collect();
    let without_pos = match words.as_slice() {
        [rest @ .., last] if !rest.is_empty() && TARGET_POS.contains(last) => rest.join(" "),
        _ => words.join(" "),
    };
    let headword = fold_for_match(headword);
    !headword.is_empty()
        && (fold_for_match(&text) == headword || fold_for_match(&without_pos) == headword)
}

/// `noun, verb` → `["noun", "verb"]`.
fn list(text: Option<String>) -> Vec<String> {
    text.map(|t| t.split(", ").map(str::to_owned).collect())
        .unwrap_or_default()
}

fn json<T: serde::de::DeserializeOwned>(text: Option<String>) -> Result<Vec<T>, DictError> {
    match text {
        None => Ok(Vec::new()),
        Some(t) => serde_json::from_str(&t).map_err(|e| DictError::Data(e.to_string())),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cross_reference_targets_name_entries() {
        assert!(names_entry("jail", "jail"));
        assert!(names_entry("programme noun", "programme"));
        assert!(names_entry("record noun (6)", "record"));
        assert!(names_entry("snib2", "snib"));
        assert!(!names_entry("hard disk", "disk"));
        assert!(!names_entry("noun", ""));
    }
}
