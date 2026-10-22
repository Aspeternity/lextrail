//! The ECDICT dictionary (`lextrail-dict-core.db`), used to tell kinds of word forms and
//! to order lemmas by frequency (`docs/specs/m1.5-oxford.md` 3, 6.1).

use std::collections::HashMap;
use std::path::Path;

use anyhow::{Context, Result};
use rusqlite::{Connection, OpenFlags};

#[derive(Debug, Default)]
pub struct Ecdict {
    /// Lowercase form → (lowercase lemma, ECDICT kind code).
    forms: HashMap<String, Vec<(String, String)>>,
    /// Lowercase word → frequency rank (`frq`, else `bnc`; smaller is more frequent).
    frequency: HashMap<String, i64>,
    /// `meta.source_commit`, `meta.selection`, `meta.format_version`, and the file's
    /// SHA-256, for the extended dictionary's `meta`.
    pub source_commit: String,
    pub selection: String,
    pub format_version: String,
    pub sha256: String,
}

impl Ecdict {
    pub fn open(path: &Path) -> Result<Self> {
        let conn = Connection::open_with_flags(path, OpenFlags::SQLITE_OPEN_READ_ONLY)
            .with_context(|| format!("opening {}", path.display()))?;
        let meta = |key: &str| -> Result<String> {
            conn.query_row("SELECT value FROM meta WHERE key = ?1", [key], |r| r.get(0))
                .with_context(|| format!("{}: meta.{key}", path.display()))
        };
        let mut ecdict = Ecdict {
            source_commit: meta("source_commit")?,
            selection: meta("selection")?,
            format_version: meta("format_version")?,
            sha256: crate::sha256_file(path)?,
            ..Ecdict::default()
        };
        let mut forms = conn.prepare("SELECT form, lemma, kind FROM forms")?;
        let rows = forms.query_map([], |r| {
            Ok((
                r.get::<_, String>(0)?,
                r.get::<_, String>(1)?,
                r.get::<_, String>(2)?,
            ))
        })?;
        for row in rows {
            let (form, lemma, kind) = row?;
            ecdict
                .forms
                .entry(form.to_lowercase())
                .or_default()
                .push((lemma.to_lowercase(), kind));
        }
        let mut words = conn.prepare("SELECT word, frq, bnc FROM entries")?;
        let rows = words.query_map([], |r| {
            Ok((
                r.get::<_, String>(0)?,
                r.get::<_, Option<i64>>(1)?,
                r.get::<_, Option<i64>>(2)?,
            ))
        })?;
        for row in rows {
            let (word, frq, bnc) = row?;
            if let Some(rank) = frq.or(bnc) {
                let slot = ecdict.frequency.entry(word.to_lowercase()).or_insert(rank);
                *slot = (*slot).min(rank);
            }
        }
        Ok(ecdict)
    }

    /// The lemmas `form` is a form of (compared lowercase), each with its ECDICT kind
    /// codes, in ECDICT order.
    pub fn lemmas(&self, form: &str) -> Vec<(String, Vec<String>)> {
        let mut out: Vec<(String, Vec<String>)> = Vec::new();
        for (lemma, kind) in self.forms.get(&form.to_lowercase()).into_iter().flatten() {
            match out.iter_mut().find(|(l, _)| l == lemma) {
                Some((_, kinds)) => kinds.push(kind.clone()),
                None => out.push((lemma.clone(), vec![kind.clone()])),
            }
        }
        out
    }

    pub fn frequency(&self, word: &str) -> Option<i64> {
        self.frequency.get(&word.to_lowercase()).copied()
    }

    /// For tests: an ECDICT with the given forms `(form, lemma, kind)` and no words.
    #[cfg(test)]
    pub fn with_forms(forms: &[(&str, &str, &str)]) -> Self {
        let mut e = Ecdict::default();
        for (form, lemma, kind) in forms {
            e.forms
                .entry(form.to_lowercase())
                .or_default()
                .push((lemma.to_lowercase(), (*kind).to_owned()));
        }
        e
    }
}
