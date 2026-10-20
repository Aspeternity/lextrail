//! Reading the dictionary file: lookup with lemmatization (`docs/specs/m1-dict.md`
//! section 8).

use std::path::{Path, PathBuf};

use rusqlite::{Connection, OpenFlags, OptionalExtension};

use crate::{Classification, FORMAT_VERSION, FormKind, Sense, suffix};

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

impl Inflection {
    /// How the form relates to its base form, for display: `leave 的过去式、过去分词`,
    /// `good 的比较级`; a phrase gives `give up 的变化形式`.
    pub fn describe_zh(&self) -> String {
        let mut kinds = self.kinds.clone();
        kinds.sort();
        kinds.dedup();
        if kinds.is_empty() {
            return format!("{} 的变化形式", self.lemma);
        }
        let names: Vec<&str> = kinds.iter().map(|k| k.name_zh()).collect();
        format!("{} 的{}", self.lemma, names.join("、"))
    }
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

/// An entry in a lookup result, with its relation to the looked-up text.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FoundEntry {
    pub entry: Entry,
    pub role: EntryRole,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum EntryRole {
    /// The looked-up word's own entry.
    Query,
    /// The looked-up word's own entry, which is also a form of another word: shown with
    /// [`Inflection::describe_zh`], e.g. "left（leave 的过去式）", because the base form's
    /// meaning is often the one meant (`found` is mostly "find" past tense, not "建立").
    FormOf(Inflection),
    /// The base form's entry.
    Lemma,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LookupResult {
    /// The text as looked up, after normalization.
    pub query: String,
    /// The text's own entry (if it says more than "past tense of ..."), then its base
    /// form's entry; the base form first when the text's entry has no exam tag, Collins
    /// stars or Oxford mark (`improving`). A word with entries in two cases (`March` / `march`) gives both: the
    /// one in the selected text's exact case first, or the lowercase one first when the
    /// word starts a sentence ([`LookupOptions::sentence_start`]), where capitalization
    /// says nothing. Ordering by meaning in context is left to the AI (M2, D019).
    pub entries: Vec<FoundEntry>,
    pub inflection: Option<Inflection>,
}

impl LookupResult {
    /// The base form, as stored in the lookup history (`lookups.lemma`).
    pub fn lemma(&self) -> &str {
        match &self.inflection {
            Some(i) => &i.lemma,
            None => &self.entries[0].entry.word,
        }
    }
}

/// Result of [`Dictionary::lookup_with_parts`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Found {
    /// The text itself is in the dictionary (possibly through its base form).
    Whole(LookupResult),
    /// The text is a hyphenated compound the dictionary does not have; these are its parts
    /// (shown under "词典没有整词，以下是各部分的释义").
    Parts(PartsResult),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PartsResult {
    pub query: String,
    /// Found parts, in order, without repeats and without function words.
    pub parts: Vec<LookupResult>,
    /// Parts that are not in the dictionary either.
    pub not_found: Vec<String>,
}

/// How to look a text up.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct LookupOptions {
    /// The text is the first word(s) of its sentence (known only when the context was
    /// captured; `lextrail_core::text::starts_sentence`). A capitalized word then lists
    /// its lowercase entry first.
    pub sentence_start: bool,
}

/// SQLite page cache for the open dictionary, in KiB. A lookup reads a few dozen pages;
/// the cache stays small because the connection is kept open while the app runs. No
/// memory mapping (`mmap_size` 0): mapped pages would count towards the app's memory.
const CACHE_KIB: i64 = 1024;

/// Base forms of a form with their frequency. The join compares without case so it can
/// use the `entries_word` index; a plain `=` scans every entry (about 30 ms).
const EXCHANGE_LEMMA_SQL: &str = "SELECT f.lemma, f.kind, e.frq, e.bnc FROM forms f
     LEFT JOIN entries e ON e.word = f.lemma COLLATE NOCASE
     WHERE f.form = ?1 COLLATE NOCASE
     ORDER BY f.lemma, f.kind";

/// Parts of a compound that get no entry of their own (`English-to-German`).
const FUNCTION_WORDS: &[&str] = &[
    "a", "an", "and", "as", "at", "but", "by", "for", "from", "in", "into", "nor", "of", "on",
    "onto", "or", "per", "the", "to", "via", "vs", "with",
];

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
        conn.execute_batch(&format!(
            "PRAGMA cache_size = -{CACHE_KIB}; PRAGMA mmap_size = 0;"
        ))?;
        Ok(Self { conn })
    }

    /// Looks up a word or phrase as a whole, with default [`LookupOptions`]; see
    /// [`Dictionary::lookup_with`].
    pub fn lookup(&self, text: &str) -> Result<Option<LookupResult>, DictError> {
        self.lookup_with(text, LookupOptions::default())
    }

    /// Looks up a word or phrase as a whole. `None` when neither it nor a base form is in
    /// the dictionary. For deciding whether to join a word broken across lines use
    /// [`Dictionary::is_word`]; never [`Dictionary::lookup_with_parts`].
    pub fn lookup_with(
        &self,
        text: &str,
        options: LookupOptions,
    ) -> Result<Option<LookupResult>, DictError> {
        let query = normalize_query(text);
        if query.is_empty() {
            return Ok(None);
        }
        let direct = self.entries_for(&query, options.sentence_start)?;
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

        // A base form is only worth reporting when its entry is shown.
        let lemma_entry = lemma_entry.filter(|l| !direct.iter().any(|d| same_word(d, l)));
        let inflection = inflection.filter(|_| lemma_entry.is_some());
        let mut entries = Vec::new();
        if !note_only || lemma_entry.is_none() {
            let role = match &inflection {
                Some(i) => EntryRole::FormOf(i.clone()),
                None => EntryRole::Query,
            };
            entries.extend(direct.into_iter().map(|entry| FoundEntry {
                entry,
                role: role.clone(),
            }));
        }
        if let Some(entry) = lemma_entry {
            // The form's own entry comes first (`better`, `left`) unless nothing marks it
            // as a common word: `improving` ("有启发的", no exam tag, no Collins stars) is
            // mostly "improve" in running text (D044 supplement).
            let at = if entries.iter().any(|f| is_marked(&f.entry)) {
                entries.len()
            } else {
                0
            };
            entries.insert(
                at,
                FoundEntry {
                    entry,
                    role: EntryRole::Lemma,
                },
            );
        }
        if entries.is_empty() {
            return Ok(None);
        }
        Ok(Some(LookupResult {
            query,
            entries,
            inflection,
        }))
    }

    /// Like [`Dictionary::lookup`], but a hyphenated compound that is not in the dictionary
    /// is split at hyphens and spaces, and its parts are looked up (skipping function words
    /// such as `to`). Text without a hyphen is never split.
    /// `None` when no part is found either.
    pub fn lookup_with_parts(
        &self,
        text: &str,
        options: LookupOptions,
    ) -> Result<Option<Found>, DictError> {
        if let Some(whole) = self.lookup_with(text, options)? {
            return Ok(Some(Found::Whole(whole)));
        }
        let query = normalize_query(text);
        if !query.contains('-') {
            return Ok(None);
        }
        let mut seen: Vec<String> = Vec::new();
        let mut parts = Vec::new();
        let mut not_found = Vec::new();
        // Spaces separate parts too once the text is split: Edge's PDF viewer gives
        // `English to-German` for a compound broken after its first hyphen.
        for part in query.split(['-', ' ']).filter(|p| !p.is_empty()) {
            let lower = part.to_lowercase();
            if FUNCTION_WORDS.contains(&lower.as_str()) || seen.contains(&lower) {
                continue;
            }
            seen.push(lower);
            match self.lookup(part)? {
                Some(r) => parts.push(r),
                None => not_found.push(part.to_owned()),
            }
        }
        if parts.is_empty() {
            return Ok(None);
        }
        Ok(Some(Found::Parts(PartsResult {
            query,
            parts,
            not_found,
        })))
    }

    /// Whether `word` is a word, for joining a word broken across lines
    /// (`lextrail_core::text::resolve_line_end_hyphens`, `join_unmarked_line_breaks`):
    /// a whole-word [`Dictionary::lookup`], else the headword list of the full ECDICT
    /// (`extra_words`, D044 supplement), directly or through a suffix rule
    /// (`convolutional`, `hyperparameters` if `hyperparameter` were listed). Compounds
    /// split into parts never count.
    pub fn is_word(&self, word: &str) -> Result<bool, DictError> {
        if self.lookup(word)?.is_some() {
            return Ok(true);
        }
        let lower = normalize_query(word).to_lowercase();
        if lower.is_empty() || !lower.bytes().all(|b| b.is_ascii_lowercase()) {
            return Ok(false);
        }
        let mut stmt = self
            .conn
            .prepare_cached("SELECT 1 FROM extra_words WHERE word = ?1")?;
        if stmt.exists([&lower])? {
            return Ok(true);
        }
        for (base, _) in suffix::candidates(&lower) {
            if stmt.exists([&base])? {
                return Ok(true);
            }
        }
        Ok(false)
    }

    /// Classifies selected text as word, phrase or sentence ([`crate::classify`]), asking
    /// this dictionary whether a multi-word text is an entry as a whole.
    pub fn classify(&self, text: &str) -> Result<Classification, DictError> {
        let mut failed = None;
        let class = crate::classify(text, |query| match self.lookup(query) {
            Ok(found) => found.is_some(),
            Err(e) => {
                failed = Some(e);
                false
            }
        });
        failed.map_or(Ok(class), Err)
    }

    /// The best entry for `word`; see [`Dictionary::entries_for`].
    fn entry(&self, word: &str) -> Result<Option<Entry>, DictError> {
        Ok(self.entries_for(word, false)?.into_iter().next())
    }

    /// Entries for `word` in any case: the exact case, else the lowercase entry, else the
    /// first by source order. A capitalized word (`March`) with its own and a lowercase
    /// entry gives both: its own first, or the lowercase one first at a sentence start.
    fn entries_for(&self, word: &str, sentence_start: bool) -> Result<Vec<Entry>, DictError> {
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
            (Some(e), Some(l)) if e.0 != l.0 && is_capitalized(word) => {
                if sentence_start {
                    vec![l.0, e.0]
                } else {
                    vec![e.0, l.0]
                }
            }
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
        let mut stmt = self.conn.prepare_cached(EXCHANGE_LEMMA_SQL)?;
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

fn same_word(a: &Entry, b: &Entry) -> bool {
    a.word.eq_ignore_ascii_case(&b.word)
}

/// Has an exam tag, Collins stars or the Oxford 3000 mark.
fn is_marked(entry: &Entry) -> bool {
    !entry.tags.is_empty() || entry.collins.is_some_and(|c| c > 0) || entry.oxford
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
        ("English", Some("'iŋgliʃ"), Some(700), &[("n.", "英语")]),
        ("German", Some("'dʒə:mən"), Some(900), &[("n.", "德语")]),
        ("self", Some("self"), Some(800), &[("n.", "自我")]),
        (
            "attention",
            Some("ə'tenʃən"),
            Some(650),
            &[("n.", "注意力")],
        ),
        (
            "computation",
            Some("ˌkɒmpju'teiʃən"),
            Some(5000),
            &[("n.", "计算")],
        ),
        ("to", Some("tu:"), Some(2), &[("prep.", "到")]),
        ("large", Some("lɑ:dʒ"), Some(200), &[("a.", "大的")]),
        ("optimize", Some("'ɒptimaiz"), None, &[("vt.", "使最优化")]),
        ("improve", Some("im'pru:v"), Some(400), &[("v.", "改善")]),
        (
            "improving",
            Some("im'pru:viŋ"),
            None,
            &[("a.", "有启发的；有教育意义的")],
        ),
    ];

    /// Exam tags; entries not listed have none.
    const TAGS: &[(&str, &str)] = &[
        ("launch", "cet4 cet6"),
        ("better", "zk gk cet4"),
        ("improve", "cet4"),
    ];

    /// Headwords of the full ECDICT that the compact dictionary lacks.
    const EXTRA_WORDS: &[&str] = &["convolutional", "hyperparameter", "salakhutdinov"];

    const FORMS: &[(&str, &str, &str)] = &[
        ("launched", "launch", "d"),
        ("launched", "launch", "p"),
        ("better", "good", "r"),
        ("gave", "give", "p"),
        ("improving", "improve", "i"),
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
                    TAGS.iter().find(|(w, _)| w == word).map(|(_, t)| *t)
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
        for word in EXTRA_WORDS {
            conn.execute("INSERT INTO extra_words (word) VALUES (?1)", [word])
                .unwrap();
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
        result
            .entries
            .iter()
            .map(|e| e.entry.word.as_str())
            .collect()
    }

    #[test]
    fn exact_and_case_folded() {
        let dir = tempfile::tempdir().unwrap();
        let dict = fixture(dir.path());
        let r = dict.lookup("launch").unwrap().unwrap();
        assert_eq!(words(&r), ["launch"]);
        assert_eq!(r.inflection, None);
        assert_eq!(r.lemma(), "launch");
        assert_eq!(r.entries[0].entry.tags, ["cet4", "cet6"]);
        assert_eq!(r.entries[0].entry.senses[1].text, "发动");
        assert_eq!(r.entries[0].role, EntryRole::Query);
        // Entries in two cases: the selected text's own case first...
        assert_eq!(words(&dict.lookup("The").unwrap().unwrap()), ["The", "the"]);
        // ...but the lowercase one first at a sentence start.
        let start = LookupOptions {
            sentence_start: true,
        };
        assert_eq!(
            words(&dict.lookup_with("The", start).unwrap().unwrap()),
            ["the", "The"]
        );
        // Lowercase text: its own entry only, at a sentence start too.
        assert_eq!(
            words(&dict.lookup_with("the", start).unwrap().unwrap()),
            ["the"]
        );
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
        assert_eq!(r.entries[0].role, EntryRole::Lemma);
        assert_eq!(
            r.inflection.unwrap().describe_zh(),
            "launch 的过去式、过去分词"
        );
    }

    #[test]
    fn form_with_its_own_meaning_shows_both() {
        let dir = tempfile::tempdir().unwrap();
        let dict = fixture(dir.path());
        let r = dict.lookup("better").unwrap().unwrap();
        assert_eq!(words(&r), ["better", "good"]);
        assert_eq!(r.lemma(), "good");
        // "better（good 的比较级）", then "good".
        match &r.entries[0].role {
            EntryRole::FormOf(i) => assert_eq!(i.describe_zh(), "good 的比较级"),
            other => panic!("{other:?}"),
        }
        assert_eq!(r.entries[1].role, EntryRole::Lemma);
        assert_eq!(r.inflection.unwrap().kinds, [FormKind::Comparative]);
    }

    #[test]
    fn unmarked_form_entry_comes_after_the_base_form() {
        let dir = tempfile::tempdir().unwrap();
        let dict = fixture(dir.path());
        // `improving` has its own entry but no exam tag, Collins stars or Oxford mark:
        // the base form first, then the form with its relation.
        let r = dict.lookup("improving").unwrap().unwrap();
        assert_eq!(words(&r), ["improve", "improving"]);
        assert_eq!(r.entries[0].role, EntryRole::Lemma);
        match &r.entries[1].role {
            EntryRole::FormOf(i) => assert_eq!(i.describe_zh(), "improve 的现在分词"),
            other => panic!("{other:?}"),
        }
        assert_eq!(r.lemma(), "improve");
    }

    #[test]
    fn any_mark_counts() {
        let plain = Entry {
            word: "x".into(),
            phonetic: None,
            tags: vec![],
            collins: None,
            oxford: false,
            bnc: None,
            frq: None,
            senses: vec![],
        };
        assert!(!is_marked(&plain));
        assert!(!is_marked(&Entry {
            collins: Some(0),
            ..plain.clone()
        }));
        assert!(is_marked(&Entry {
            collins: Some(1),
            ..plain.clone()
        }));
        assert!(is_marked(&Entry {
            oxford: true,
            ..plain.clone()
        }));
        assert!(is_marked(&Entry {
            tags: vec!["gre".into()],
            ..plain
        }));
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
    fn whole_compounds_are_not_split() {
        let dir = tempfile::tempdir().unwrap();
        let dict = fixture(dir.path());
        // A word found as a whole never goes through the parts path.
        assert!(matches!(
            dict.lookup_with_parts("launch", LookupOptions::default())
                .unwrap(),
            Some(Found::Whole(_))
        ));
        assert_eq!(
            dict.lookup_with_parts("tokenizer", LookupOptions::default())
                .unwrap(),
            None
        );
    }

    #[test]
    fn missing_compounds_are_split_into_parts() {
        let dir = tempfile::tempdir().unwrap();
        let dict = fixture(dir.path());
        // Not in the dictionary as a whole...
        assert_eq!(dict.lookup("English-to-German").unwrap(), None);
        // ...so its parts, without the function word `to`.
        let Some(Found::Parts(p)) = dict
            .lookup_with_parts("English-to-German", LookupOptions::default())
            .unwrap()
        else {
            panic!("expected parts");
        };
        assert_eq!(p.query, "English-to-German");
        let parts: Vec<&str> = p.parts.iter().map(|r| words(r)[0]).collect();
        assert_eq!(parts, ["English", "German"]);
        assert!(p.not_found.is_empty());

        // Repeated parts once; parts are lemmatized; unknown parts are listed.
        let Some(Found::Parts(p)) = dict
            .lookup_with_parts("models-of-models-qux", LookupOptions::default())
            .unwrap()
        else {
            panic!("expected parts");
        };
        assert_eq!(p.parts.len(), 1);
        assert_eq!(p.parts[0].lemma(), "model");
        assert_eq!(p.not_found, ["qux"]);

        // Edge's PDF viewer: `English to-German`, the first hyphen lost at a line end.
        // Once split, spaces separate parts too.
        let Some(Found::Parts(p)) = dict
            .lookup_with_parts("English to-German", LookupOptions::default())
            .unwrap()
        else {
            panic!("expected parts");
        };
        let parts: Vec<&str> = p.parts.iter().map(|r| words(r)[0]).collect();
        assert_eq!(parts, ["English", "German"]);
        assert!(p.not_found.is_empty());
        // Without a hyphen a phrase is never split.
        assert_eq!(
            dict.lookup_with_parts("English to German", LookupOptions::default())
                .unwrap(),
            None
        );

        // No part found: nothing.
        assert_eq!(
            dict.lookup_with_parts("foo-to-bar", LookupOptions::default())
                .unwrap(),
            None
        );
        // Only function words: nothing.
        assert_eq!(
            dict.lookup_with_parts("to-the", LookupOptions::default())
                .unwrap(),
            None
        );
    }

    #[test]
    fn line_end_hyphens_join_only_whole_words() {
        use lextrail_core::text::{LINE_END_HYPHEN, resolve_line_end_hyphens};
        let dir = tempfile::tempdir().unwrap();
        let dict = fixture(dir.path());
        // The desktop's check: whole words and the full headword list, never parts.
        let is_word = |w: &str| dict.is_word(w).unwrap();
        let m = LINE_END_HYPHEN;
        // A hyphenation break is joined...
        assert_eq!(
            resolve_line_end_hyphens(&format!("sequential compu{m}tation of"), is_word),
            "sequential computation of"
        );
        // ...a compound's hyphen is kept, although its parts are in the dictionary.
        let kept = format!("English{m}to-German");
        assert_eq!(resolve_line_end_hyphens(&kept, is_word), kept);
        let kept = format!("self{m}attention");
        assert_eq!(resolve_line_end_hyphens(&kept, is_word), kept);
        assert!(matches!(
            dict.lookup_with_parts("self-attention", LookupOptions::default())
                .unwrap(),
            Some(Found::Parts(_))
        ));
        // A word only in the full ECDICT's headword list (D044 supplement).
        assert_eq!(
            resolve_line_end_hyphens(&format!("and convolu{m}tional layers"), is_word),
            "and convolutional layers"
        );
    }

    #[test]
    fn words_for_joining_include_the_full_headword_list() {
        let dir = tempfile::tempdir().unwrap();
        let dict = fixture(dir.path());
        assert!(dict.is_word("computation").unwrap());
        assert!(dict.is_word("launched").unwrap());
        assert!(dict.is_word("Convolutional").unwrap());
        assert!(dict.is_word("Salakhutdinov").unwrap());
        // Suffix rules on the headword list too.
        assert!(dict.is_word("hyperparameters").unwrap());
        // Not words: fragments, compounds split into parts, non-letters.
        for not in [
            "convolu",
            "tional",
            "self-attention",
            "selfattention",
            "2014",
            "",
        ] {
            assert!(!dict.is_word(not).unwrap(), "{not}");
        }
        // Only for joining: lookups do not see the headword list.
        assert_eq!(dict.lookup("convolutional").unwrap(), None);
    }

    #[test]
    fn classifies_with_this_dictionary() {
        use lextrail_core::lookup::LookupKind;
        let dir = tempfile::tempdir().unwrap();
        let dict = fixture(dir.path());
        let english = |k| Ok::<_, ()>(Classification::English(k));
        let classify = |t: &str| dict.classify(t).map_err(|_| ());
        assert_eq!(classify("launched"), english(LookupKind::Word));
        assert_eq!(classify("gave up."), english(LookupKind::Phrase));
        assert_eq!(classify("We give up."), english(LookupKind::Sentence));
        assert_eq!(classify("注意力"), Ok(Classification::NotEnglish));
    }

    #[test]
    fn look_up_gives_kind_and_lemma_for_the_history() {
        use crate::view::LookupView;
        use lextrail_core::lookup::LookupKind;
        let dir = tempfile::tempdir().unwrap();
        let dict = fixture(dir.path());
        let look = |text: &str| {
            let l = dict.look_up(text, LookupOptions::default()).unwrap();
            (l.kind, l.lemma, l.view)
        };
        let (kind, lemma, view) = look("launched");
        assert_eq!(
            (kind, lemma.as_deref()),
            (Some(LookupKind::Word), Some("launch"))
        );
        assert!(matches!(view, LookupView::Entries { .. }));
        let (kind, lemma, _) = look("gave up");
        assert_eq!(
            (kind, lemma.as_deref()),
            (Some(LookupKind::Phrase), Some("give up"))
        );
        // Found only as parts, not found, a sentence: no base form.
        let (kind, lemma, view) = look("English-to-German");
        assert_eq!((kind, lemma), (Some(LookupKind::Word), None));
        assert!(matches!(view, LookupView::Parts { .. }));
        let (kind, lemma, view) = look("tokenizer");
        assert_eq!(
            (kind, lemma, view),
            (Some(LookupKind::Word), None, LookupView::NotFound)
        );
        let (kind, lemma, view) = look("We give up.");
        assert_eq!(
            (kind, lemma, view),
            (Some(LookupKind::Sentence), None, LookupView::Sentence)
        );
        // Not recorded at all (D045).
        assert_eq!(look("注意力"), (None, None, LookupView::NotEnglish));
    }

    #[test]
    fn base_form_query_uses_the_index() {
        let dir = tempfile::tempdir().unwrap();
        let dict = fixture(dir.path());
        let plan: Vec<String> = dict
            .conn
            .prepare(&format!("EXPLAIN QUERY PLAN {EXCHANGE_LEMMA_SQL}"))
            .unwrap()
            .query_map(["left"], |r| r.get(3))
            .unwrap()
            .collect::<Result<_, _>>()
            .unwrap();
        assert!(
            plan.iter().all(|step| !step.starts_with("SCAN")),
            "{plan:?}"
        );
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
