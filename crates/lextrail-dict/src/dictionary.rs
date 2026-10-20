//! Reading the dictionary file: lookup with lemmatization (`docs/specs/m1-dict.md`
//! section 8).

use std::path::{Path, PathBuf};

use rusqlite::{Connection, OpenFlags, OptionalExtension};

use crate::{FORMAT_VERSION, FormKind, Sense, suffix};

#[derive(Debug, thiserror::Error)]
pub enum DictError {
    #[error("dictionary file not found: {0}")]
    NotFound(PathBuf),
    #[error("dictionary format {found:?} is not supported (this build reads {FORMAT_VERSION})")]
    UnsupportedFormat { found: Option<String> },
    #[error(transparent)]
    Sqlite(#[from] rusqlite::Error),
}

/// A dictionary entry with its senses in source order.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Entry {
    pub word: String,
    pub phonetic: Option<String>,
    /// Exam tags, e.g. `cet4`, `toefl`.
    pub tags: Vec<String>,
    pub collins: Option<u8>,
    pub oxford: bool,
    pub bnc: Option<u32>,
    pub frq: Option<u32>,
    pub senses: Vec<Sense>,
}

/// How the looked-up text relates to its base form.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Inflection {
    /// The text as looked up (normalized).
    pub form: String,
    pub lemma: String,
    /// Kinds of form, e.g. past tense; empty for phrases.
    pub kinds: Vec<FormKind>,
    pub source: InflectionSource,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InflectionSource {
    /// ECDICT's `exchange` data (the `forms` table).
    Exchange,
    /// English suffix rules, for forms `exchange` does not list.
    SuffixRule,
    /// A phrase whose words were each reduced to their base form (`gave up` -> `give up`).
    PhraseWords,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LookupResult {
    /// The text as looked up, after normalization.
    pub query: String,
    /// The text's own entry (if it says more than "past tense of ..."), then its base
    /// form's entry. A capitalized word with both a lowercase and a capitalized entry gives
    /// both, lowercase first: `March` is "三月" mid-sentence but "行军" at a sentence start.
    pub entries: Vec<Entry>,
    pub inflection: Option<Inflection>,
}

impl LookupResult {
    /// The base form, as stored in the lookup history (`lookups.lemma`).
    pub fn lemma(&self) -> &str {
        match &self.inflection {
            Some(i) => &i.lemma,
            None => &self.entries[0].word,
        }
    }
}

/// An open dictionary file, read-only. Not `Sync`: share it behind a mutex.
#[derive(Debug)]
pub struct Dictionary {
    conn: Connection,
}

impl Dictionary {
    /// Opens a dictionary file and checks its format version.
    pub fn open(path: &Path) -> Result<Self, DictError> {
        if !path.is_file() {
            return Err(DictError::NotFound(path.to_owned()));
        }
        let conn = Connection::open_with_flags(
            path,
            OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_NO_MUTEX,
        )?;
        let found: Option<String> = conn
            .query_row(
                "SELECT value FROM meta WHERE key = 'format_version'",
                [],
                |r| r.get(0),
            )
            .optional()
            .unwrap_or(None);
        if found.as_deref() != Some(&FORMAT_VERSION.to_string()) {
            return Err(DictError::UnsupportedFormat { found });
        }
        Ok(Self { conn })
    }

    /// Looks up a word or phrase. `None` when neither it nor a base form is in the
    /// dictionary.
    pub fn lookup(&self, text: &str) -> Result<Option<LookupResult>, DictError> {
        let query = normalize_query(text);
        if query.is_empty() {
            return Ok(None);
        }
        let direct = self.entries_for(&query)?;
        let mut inflection = self.exchange_lemma(&query)?;
        let note_only = direct.first().is_some_and(is_form_note);
        if inflection.is_none() && (direct.is_empty() || note_only) {
            inflection = if query.contains(' ') {
                self.phrase_lemma(&query)?
            } else {
                self.suffix_lemma(&query)?
            };
        }
        let lemma_entry = match &inflection {
            Some(i) => self.entry(&i.lemma)?,
            None => None,
        };

        let mut entries = Vec::new();
        if !note_only || lemma_entry.is_none() {
            entries.extend(direct);
        }
        if let Some(l) = lemma_entry
            && !entries.iter().any(|e| e.word.eq_ignore_ascii_case(&l.word))
        {
            entries.push(l);
        }
        if entries.is_empty() {
            return Ok(None);
        }
        // A base form is only worth reporting when its entry is shown.
        let inflection = inflection.filter(|i| {
            entries
                .iter()
                .any(|e| e.word.eq_ignore_ascii_case(&i.lemma))
        });
        Ok(Some(LookupResult {
            query,
            entries,
            inflection,
        }))
    }

    /// The best entry for `word`; see [`Dictionary::entries_for`].
    fn entry(&self, word: &str) -> Result<Option<Entry>, DictError> {
        Ok(self.entries_for(word)?.into_iter().next())
    }

    /// Entries for `word` in any case: the exact case, else the lowercase entry, else the
    /// first by source order. A capitalized word (`March`) gives the lowercase entry and
    /// then its own, when both exist.
    fn entries_for(&self, word: &str) -> Result<Vec<Entry>, DictError> {
        let mut stmt = self.conn.prepare_cached(
            "SELECT id, word FROM entries WHERE word = ?1 COLLATE NOCASE ORDER BY id",
        )?;
        let hits = stmt
            .query_map([word], |r| {
                Ok((r.get::<_, i64>(0)?, r.get::<_, String>(1)?))
            })?
            .collect::<Result<Vec<_>, _>>()?;
        let lower = word.to_lowercase();
        let exact = hits.iter().find(|(_, w)| w == word);
        let lowercase = hits.iter().find(|(_, w)| *w == lower);
        let ids: Vec<i64> = match (exact, lowercase) {
            (Some(e), Some(l)) if e.0 != l.0 && is_capitalized(word) => vec![l.0, e.0],
            (Some(e), _) => vec![e.0],
            (None, Some(l)) => vec![l.0],
            (None, None) => hits.first().map(|h| h.0).into_iter().collect(),
        };
        ids.into_iter().map(|id| self.load(id)).collect()
    }

    fn load(&self, id: i64) -> Result<Entry, DictError> {
        let mut entry = self.conn.query_row(
            "SELECT word, phonetic, collins, oxford, bnc, frq, tags FROM entries WHERE id = ?1",
            [id],
            |r| {
                Ok(Entry {
                    word: r.get(0)?,
                    phonetic: r.get(1)?,
                    collins: r.get(2)?,
                    oxford: r.get::<_, Option<i64>>(3)? == Some(1),
                    bnc: r.get(4)?,
                    frq: r.get(5)?,
                    tags: r
                        .get::<_, Option<String>>(6)?
                        .map(|t| t.split(' ').map(str::to_owned).collect())
                        .unwrap_or_default(),
                    senses: Vec::new(),
                })
            },
        )?;
        let mut stmt = self.conn.prepare_cached(
            "SELECT pos, domain, text FROM senses WHERE entry_id = ?1 ORDER BY ord",
        )?;
        entry.senses = stmt
            .query_map([id], |r| {
                Ok(Sense {
                    pos: r.get(0)?,
                    domain: r.get(1)?,
                    text: r.get(2)?,
                })
            })?
            .collect::<Result<_, _>>()?;
        Ok(entry)
    }

    /// Base form from the `forms` table. Several lemmas (e.g. `found` -> find / found):
    /// the more frequent one.
    fn exchange_lemma(&self, form: &str) -> Result<Option<Inflection>, DictError> {
        let mut stmt = self.conn.prepare_cached(
            "SELECT f.lemma, f.kind, e.frq, e.bnc FROM forms f
             LEFT JOIN entries e ON e.word = f.lemma
             WHERE f.form = ?1 COLLATE NOCASE
             ORDER BY f.lemma, f.kind",
        )?;
        let rows = stmt
            .query_map([form], |r| {
                Ok((
                    r.get::<_, String>(0)?,
                    r.get::<_, String>(1)?,
                    r.get::<_, Option<u32>>(2)?,
                    r.get::<_, Option<u32>>(3)?,
                ))
            })?
            .collect::<Result<Vec<_>, _>>()?;
        // Lower rank = more frequent; unranked last.
        let rank = |frq: Option<u32>, bnc: Option<u32>| frq.or(bnc).unwrap_or(u32::MAX);
        let Some(lemma) = rows
            .iter()
            .filter(|(lemma, ..)| !lemma.eq_ignore_ascii_case(form))
            .min_by_key(|(_, _, frq, bnc)| rank(*frq, *bnc))
            .map(|(lemma, ..)| lemma.clone())
        else {
            return Ok(None);
        };
        let mut kinds: Vec<FormKind> = rows
            .iter()
            .filter(|(l, ..)| *l == lemma)
            .filter_map(|(_, kind, ..)| FormKind::from_code(kind))
            .collect();
        kinds.dedup();
        Ok(Some(Inflection {
            form: form.to_owned(),
            lemma,
            kinds,
            source: InflectionSource::Exchange,
        }))
    }

    fn suffix_lemma(&self, word: &str) -> Result<Option<Inflection>, DictError> {
        for (base, kinds) in suffix::candidates(&word.to_lowercase()) {
            // `-er` / `-est` are comparison only for adjectives: `optimizer` is not a
            // comparative of `optimize`.
            let comparison = kinds
                .iter()
                .any(|k| matches!(k, FormKind::Comparative | FormKind::Superlative));
            if let Some(entry) = self.entry(&base)?
                && !is_form_note(&entry)
                && (!comparison || is_adjective(&entry))
            {
                return Ok(Some(Inflection {
                    form: word.to_owned(),
                    lemma: entry.word,
                    kinds: kinds.to_vec(),
                    source: InflectionSource::SuffixRule,
                }));
            }
        }
        Ok(None)
    }

    /// `gave up` -> `give up`: each word's base form, if the result is an entry.
    fn phrase_lemma(&self, phrase: &str) -> Result<Option<Inflection>, DictError> {
        let mut words = Vec::new();
        for word in phrase.split(' ') {
            let base = match self.exchange_lemma(word)? {
                Some(i) => i.lemma,
                None => word.to_owned(),
            };
            words.push(base);
        }
        let lemma = words.join(" ");
        if lemma.eq_ignore_ascii_case(phrase) || self.entry(&lemma)?.is_none() {
            return Ok(None);
        }
        Ok(Some(Inflection {
            form: phrase.to_owned(),
            lemma,
            kinds: Vec::new(),
            source: InflectionSource::PhraseWords,
        }))
    }
}

/// Trims, unifies apostrophes, drops surrounding punctuation and a possessive `'s`, and
/// collapses whitespace.
pub fn normalize_query(text: &str) -> String {
    let text = text.replace(['\u{2019}', '\u{2018}'], "'");
    let words: Vec<&str> = text.split_whitespace().collect();
    let joined = words.join(" ");
    let trimmed = joined.trim_matches(|c: char| !c.is_alphanumeric());
    let trimmed = trimmed.strip_suffix("'s").unwrap_or(trimmed);
    trimmed.to_owned()
}

/// Has an adjective sense (`a.`, `adj.`, or a joined POS such as `a. & n.`).
fn is_adjective(entry: &Entry) -> bool {
    entry.senses.iter().any(|s| {
        s.pos
            .as_deref()
            .is_some_and(|p| p.split(" & ").any(|t| t == "a." || t == "adj."))
    })
}

/// `March`: an uppercase first letter and lowercase letters after it.
fn is_capitalized(word: &str) -> bool {
    let mut chars = word.chars();
    chars.next().is_some_and(char::is_uppercase) && chars.all(|c| !c.is_uppercase())
}

/// Every sense only says which form of another word this is ("launch的过去式和过去分词").
fn is_form_note(entry: &Entry) -> bool {
    const NOTES: [&str; 7] = [
        "的过去式",
        "的过去分词",
        "的现在分词",
        "的复数",
        "的第三人称单数",
        "的比较级",
        "的最高级",
    ];
    !entry.senses.is_empty()
        && entry
            .senses
            .iter()
            .all(|s| NOTES.iter().any(|n| s.text.contains(n)))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::schema::{INDEXES, SCHEMA};
    use rusqlite::params;

    /// word, phonetic, frq, senses (pos, text)
    type Row<'a> = (
        &'a str,
        Option<&'a str>,
        Option<u32>,
        &'a [(&'a str, &'a str)],
    );

    const ENTRIES: &[Row] = &[
        (
            "launch",
            Some("lɔ:ntʃ"),
            Some(1789),
            &[("n.", "发射"), ("vt.", "发动")],
        ),
        (
            "launched",
            Some("lɔ:ntʃt"),
            None,
            &[("v.", "发射( launch的过去式和过去分词 )")],
        ),
        ("good", Some("gud"), Some(80), &[("a.", "好的")]),
        (
            "better",
            Some("'betə"),
            Some(300),
            &[("a.", "更好的"), ("v.", "改善")],
        ),
        ("the", Some("ðə"), Some(1), &[("art.", "这")]),
        ("The", None, None, &[("", "(人名) 西")]),
        ("US", None, None, &[("abbr.", "美国")]),
        ("us", Some("ʌs"), Some(100), &[("pron.", "我们")]),
        (
            "transformer",
            Some("træns'fɔ:mə"),
            None,
            &[("n.", "变压器")],
        ),
        ("stop", Some("stɒp"), Some(400), &[("v.", "停止")]),
        ("model", Some("'mɒdl"), Some(500), &[("n.", "模型")]),
        ("give", Some("giv"), Some(90), &[("v.", "给")]),
        ("give up", None, None, &[("", "放弃")]),
        ("study", Some("'stʌdi"), Some(600), &[("v.", "学习")]),
        ("large", Some("lɑ:dʒ"), Some(200), &[("a.", "大的")]),
        ("optimize", Some("'ɒptimaiz"), None, &[("vt.", "使最优化")]),
    ];

    const FORMS: &[(&str, &str, &str)] = &[
        ("launched", "launch", "d"),
        ("launched", "launch", "p"),
        ("better", "good", "r"),
        ("gave", "give", "p"),
    ];

    fn fixture(dir: &Path) -> Dictionary {
        let path = dir.join("dict.db");
        let conn = Connection::open(&path).unwrap();
        conn.execute_batch(SCHEMA).unwrap();
        conn.execute(
            "INSERT INTO meta (key, value) VALUES ('format_version', ?1)",
            [FORMAT_VERSION.to_string()],
        )
        .unwrap();
        for (i, (word, phonetic, frq, senses)) in ENTRIES.iter().enumerate() {
            let id = i as i64 + 1;
            conn.execute(
                "INSERT INTO entries (id, word, phonetic, frq, tags) VALUES (?1, ?2, ?3, ?4, ?5)",
                params![
                    id,
                    word,
                    phonetic,
                    frq,
                    (*word == "launch").then_some("cet4 cet6")
                ],
            )
            .unwrap();
            for (ord, (pos, text)) in senses.iter().enumerate() {
                let pos = Some(*pos).filter(|p| !p.is_empty());
                conn.execute(
                    "INSERT INTO senses (entry_id, ord, pos, text) VALUES (?1, ?2, ?3, ?4)",
                    params![id, ord as i64, pos, text],
                )
                .unwrap();
            }
        }
        for (form, lemma, kind) in FORMS {
            conn.execute(
                "INSERT INTO forms (form, lemma, kind) VALUES (?1, ?2, ?3)",
                params![form, lemma, kind],
            )
            .unwrap();
        }
        conn.execute_batch(INDEXES).unwrap();
        drop(conn);
        Dictionary::open(&path).unwrap()
    }

    fn words(result: &LookupResult) -> Vec<&str> {
        result.entries.iter().map(|e| e.word.as_str()).collect()
    }

    #[test]
    fn exact_and_case_folded() {
        let dir = tempfile::tempdir().unwrap();
        let dict = fixture(dir.path());
        let r = dict.lookup("launch").unwrap().unwrap();
        assert_eq!(words(&r), ["launch"]);
        assert_eq!(r.inflection, None);
        assert_eq!(r.lemma(), "launch");
        assert_eq!(r.entries[0].tags, ["cet4", "cet6"]);
        assert_eq!(r.entries[0].senses[1].text, "发动");
        // Capitalized: the lowercase entry first, then the capitalized one (a name).
        assert_eq!(words(&dict.lookup("The").unwrap().unwrap()), ["the", "The"]);
        assert_eq!(words(&dict.lookup("THE").unwrap().unwrap()), ["the"]);
        assert_eq!(words(&dict.lookup("LAUNCH").unwrap().unwrap()), ["launch"]);
        // Exact case wins.
        assert_eq!(words(&dict.lookup("US").unwrap().unwrap()), ["US"]);
        assert_eq!(words(&dict.lookup("us").unwrap().unwrap()), ["us"]);
    }

    #[test]
    fn form_with_only_a_note_shows_the_base_form() {
        let dir = tempfile::tempdir().unwrap();
        let dict = fixture(dir.path());
        let r = dict.lookup("launched").unwrap().unwrap();
        assert_eq!(words(&r), ["launch"]);
        assert_eq!(
            r.inflection,
            Some(Inflection {
                form: "launched".into(),
                lemma: "launch".into(),
                kinds: vec![FormKind::PastParticiple, FormKind::Past],
                source: InflectionSource::Exchange,
            })
        );
        assert_eq!(r.lemma(), "launch");
    }

    #[test]
    fn form_with_its_own_meaning_shows_both() {
        let dir = tempfile::tempdir().unwrap();
        let dict = fixture(dir.path());
        let r = dict.lookup("better").unwrap().unwrap();
        assert_eq!(words(&r), ["better", "good"]);
        assert_eq!(r.lemma(), "good");
        assert_eq!(r.inflection.unwrap().kinds, [FormKind::Comparative]);
    }

    #[test]
    fn suffix_rules_cover_forms_exchange_misses() {
        let dir = tempfile::tempdir().unwrap();
        let dict = fixture(dir.path());
        for (form, lemma) in [
            ("transformers", "transformer"),
            ("stopped", "stop"),
            ("models", "model"),
            ("studies", "study"),
            ("Models", "model"),
            ("larger", "large"),
        ] {
            let r = dict.lookup(form).unwrap().unwrap();
            assert_eq!(words(&r), [lemma], "{form}");
            assert_eq!(r.inflection.unwrap().source, InflectionSource::SuffixRule);
        }
    }

    #[test]
    fn phrases_by_their_words_base_forms() {
        let dir = tempfile::tempdir().unwrap();
        let dict = fixture(dir.path());
        let r = dict.lookup("gave  up").unwrap().unwrap();
        assert_eq!(r.query, "gave up");
        assert_eq!(words(&r), ["give up"]);
        assert_eq!(r.inflection.unwrap().source, InflectionSource::PhraseWords);
        assert_eq!(
            words(&dict.lookup("give up").unwrap().unwrap()),
            ["give up"]
        );
    }

    #[test]
    fn punctuation_and_possessives_are_ignored() {
        let dir = tempfile::tempdir().unwrap();
        let dict = fixture(dir.path());
        assert_eq!(
            words(&dict.lookup("“model’s”,").unwrap().unwrap()),
            ["model"]
        );
        assert_eq!(
            words(&dict.lookup("(launch).").unwrap().unwrap()),
            ["launch"]
        );
        assert_eq!(normalize_query("  give \n up "), "give up");
        assert_eq!(normalize_query("..."), "");
    }

    #[test]
    fn unknown_text_gives_none() {
        let dir = tempfile::tempdir().unwrap();
        let dict = fixture(dir.path());
        assert_eq!(dict.lookup("tokenizer").unwrap(), None);
        assert_eq!(dict.lookup("  ").unwrap(), None);
        assert_eq!(dict.lookup("take up").unwrap(), None);
        // Not a comparative: `optimize` has no adjective sense.
        assert_eq!(dict.lookup("optimizer").unwrap(), None);
    }

    #[test]
    fn refuses_unknown_formats_and_missing_files() {
        let dir = tempfile::tempdir().unwrap();
        let missing = dir.path().join("none.db");
        assert!(matches!(
            Dictionary::open(&missing),
            Err(DictError::NotFound(_))
        ));

        let path = dir.path().join("future.db");
        let conn = Connection::open(&path).unwrap();
        conn.execute_batch(SCHEMA).unwrap();
        conn.execute(
            "INSERT INTO meta (key, value) VALUES ('format_version', '999')",
            [],
        )
        .unwrap();
        drop(conn);
        assert!(matches!(
            Dictionary::open(&path),
            Err(DictError::UnsupportedFormat { found: Some(v) }) if v == "999"
        ));

        let path = dir.path().join("not-a-dict.db");
        Connection::open(&path)
            .unwrap()
            .execute_batch("CREATE TABLE x (y)")
            .unwrap();
        assert!(matches!(
            Dictionary::open(&path),
            Err(DictError::UnsupportedFormat { found: None })
        ));
    }
}
