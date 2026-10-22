//! Looking text up in ECDICT and an extended dictionary (Oxford) together
//! (`docs/specs/m1.5-oxford.md` 6.2–6.4). Without an extended dictionary every result is
//! exactly the ECDICT one of [`Dictionary::lookup_with`] (M1).

use std::collections::HashSet;

use crate::dictionary::{FUNCTION_WORDS, is_capitalized, is_form_note, is_marked};
use crate::ext::reader::{ExtDictionary, ExtEntry, KeyRow, Relation};
use crate::{
    Classification, DictError, Dictionary, Entry, EntryRole, FormKind, LookupOptions, LookupResult,
    normalize_query,
};

/// An Oxford entry with what ECDICT adds to it (spec 6.4).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OxfordEntry {
    pub entry: ExtEntry,
    /// Exam tags of the ECDICT entry with the same headword.
    pub tags: Vec<String>,
    /// Collins stars of that entry.
    pub collins: Option<u8>,
    /// ECDICT's Oxford 3000 mark: counts as a mark (6.3) but is not shown (Oxford's own
    /// 3000 / 5000 is more accurate).
    pub ecdict_oxford: bool,
    /// ECDICT's phonetic, when the Oxford entry has none.
    pub ecdict_phonetic: Option<String>,
}

impl OxfordEntry {
    /// Oxford's marks (entry-level CEFR, Oxford 3000 / 5000, OPAL) or ECDICT's (exam tags,
    /// Collins stars, Oxford mark); sense-level CEFR does not count (spec 6.3).
    pub fn is_marked(&self) -> bool {
        self.entry.is_marked()
            || !self.tags.is_empty()
            || self.collins.is_some_and(|c| c > 0)
            || self.ecdict_oxford
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Content {
    Ecdict(Entry),
    Oxford(Box<OxfordEntry>),
}

impl Content {
    pub fn word(&self) -> &str {
        match self {
            Self::Ecdict(e) => &e.word,
            Self::Oxford(o) => &o.entry.headword,
        }
    }

    fn is_marked(&self) -> bool {
        match self {
            Self::Ecdict(e) => is_marked(e),
            Self::Oxford(o) => o.is_marked(),
        }
    }
}

/// What an entry is to the looked-up text (spec 6.3).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Role {
    /// The text's own entry.
    Query,
    /// The text's own entry, while the text is also a form, variant or derivative of
    /// another entry: shown with its relation ("found（find 的过去式、过去分词）").
    FormOf,
    /// The entry the text is a form, variant, derivative, idiom or phrasal verb of.
    Lemma,
    /// Found through an `other` relation: shown only when nothing else is found.
    Related,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Hit {
    pub content: Content,
    pub role: Role,
    /// How the text relates to a base form, for display: "find 的过去式、过去分词",
    /// "color 的另一种写法", "put 词条中的短语动词". On a `FormOf` hit it names the other
    /// entry; on a `Lemma` hit, this one.
    pub relation: Option<String>,
    /// The idiom, phrasal verb or derivative group the text leads to, shown open and first
    /// (spec 7.3).
    pub open_group: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Merged {
    /// The text as looked up, after normalization.
    pub query: String,
    /// In display order.
    pub hits: Vec<Hit>,
    /// The base form, as stored in the lookup history (`lookups.lemma`).
    pub lemma: String,
}

impl Merged {
    /// Lines shown above the entries when the text has no entry of its own: "went 是 go
    /// 的过去式"; one per distinct relation ("leaves 是 leaf 的复数").
    pub fn notes(&self) -> Vec<String> {
        if self
            .hits
            .iter()
            .any(|h| matches!(h.role, Role::Query | Role::FormOf))
        {
            return Vec::new();
        }
        let mut notes: Vec<String> = Vec::new();
        for hit in self.hits.iter().filter(|h| h.role == Role::Lemma) {
            if let Some(r) = &hit.relation {
                let note = format!("{} 是 {r}", self.query);
                if !notes.contains(&note) {
                    notes.push(note);
                }
            }
        }
        notes
    }
}

/// Result of [`Lexicon::lookup_with_parts`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MergedFound {
    Whole(Merged),
    /// A hyphenated compound neither dictionary has, split into its parts.
    Parts {
        query: String,
        parts: Vec<Merged>,
        not_found: Vec<String>,
    },
}

/// The ECDICT dictionary and, when it is installed and turned on, an extended dictionary.
#[derive(Debug, Clone, Copy)]
pub struct Lexicon<'a> {
    pub ecdict: &'a Dictionary,
    pub ext: Option<&'a ExtDictionary>,
}

impl Lexicon<'_> {
    /// Looks a word or phrase up as a whole (spec 6.2): Oxford keys first; a key Oxford
    /// does not have, or has only through `other` relations, goes through ECDICT's lookup
    /// with its base form taken from Oxford when Oxford has it; `other` entries only when
    /// nothing else is found.
    pub fn lookup_with(
        &self,
        text: &str,
        options: LookupOptions,
    ) -> Result<Option<Merged>, DictError> {
        let Some(ext) = self.ext else {
            return Ok(self.ecdict.lookup_with(text, options)?.map(from_ecdict));
        };
        let query = normalize_query(text);
        if query.is_empty() {
            return Ok(None);
        }
        let rows = oxford_rows(ext, &query)?;
        if rows.iter().any(|r| r.relation != Relation::Other)
            && let Some(found) = self.oxford_result(ext, &query, &rows, options)?
        {
            return Ok(Some(found));
        }
        if let Some(found) = self.ecdict.lookup_with(text, options)? {
            return Ok(Some(self.with_oxford_lemma(ext, found)?));
        }
        if rows.is_empty() {
            return Ok(None);
        }
        let mut seen = HashSet::new();
        let mut hits = Vec::new();
        for row in rows.iter().filter(|r| seen.insert(r.entry_id)) {
            hits.push(Hit {
                content: self.oxford(ext, row.entry_id)?,
                role: Role::Related,
                relation: None,
                open_group: None,
            });
        }
        Ok(Some(Merged {
            query: query.clone(),
            hits,
            lemma: query,
        }))
    }

    /// Like [`Lexicon::lookup_with`], but a hyphenated compound that is not found is split
    /// and its parts are looked up (as [`Dictionary::lookup_with_parts`]; Oxford first).
    pub fn lookup_with_parts(
        &self,
        text: &str,
        options: LookupOptions,
    ) -> Result<Option<MergedFound>, DictError> {
        if let Some(whole) = self.lookup_with(text, options)? {
            return Ok(Some(MergedFound::Whole(whole)));
        }
        let query = normalize_query(text);
        if !query.contains('-') {
            return Ok(None);
        }
        let mut seen: Vec<String> = Vec::new();
        let mut parts = Vec::new();
        let mut not_found = Vec::new();
        for part in query.split(['-', ' ']).filter(|p| !p.is_empty()) {
            let lower = part.to_lowercase();
            if FUNCTION_WORDS.contains(&lower.as_str()) || seen.contains(&lower) {
                continue;
            }
            seen.push(lower);
            match self.lookup_with(part, LookupOptions::default())? {
                Some(r) => parts.push(r),
                None => not_found.push(part.to_owned()),
            }
        }
        if parts.is_empty() {
            return Ok(None);
        }
        Ok(Some(MergedFound::Parts {
            query,
            parts,
            not_found,
        }))
    }

    /// Whether `word` is a word, for joining a word broken across lines: an Oxford key
    /// (any relation), else [`Dictionary::is_word`].
    pub fn is_word(&self, word: &str) -> Result<bool, DictError> {
        if let Some(ext) = self.ext
            && ext.has_key(&normalize_query(word))?
        {
            return Ok(true);
        }
        self.ecdict.is_word(word)
    }

    /// Classifies text (D045); Oxford's multi-word keys (idioms, phrasal verbs, compounds)
    /// count as dictionary phrases.
    pub fn classify(&self, text: &str) -> Result<Classification, DictError> {
        let mut failed = None;
        let class = crate::classify(text, |query| {
            let found = match self.ext {
                Some(ext) => oxford_rows(ext, &normalize_query(query)).map(|r| !r.is_empty()),
                None => Ok(false),
            }
            .and_then(|oxford| Ok(oxford || self.ecdict.lookup(query)?.is_some()));
            found.unwrap_or_else(|e| {
                failed = Some(e);
                false
            })
        });
        failed.map_or(Ok(class), Err)
    }

    /// Spec 6.2 step 2 and 6.3, for a key Oxford has with at least one relation besides
    /// `other`. `None` when nothing is left to show.
    fn oxford_result(
        &self,
        ext: &ExtDictionary,
        query: &str,
        rows: &[KeyRow],
        options: LookupOptions,
    ) -> Result<Option<Merged>, DictError> {
        let mut seen = HashSet::new();
        let rows: Vec<&KeyRow> = rows.iter().filter(|r| seen.insert(r.entry_id)).collect();
        let of = |wanted: &[Relation]| -> Vec<&KeyRow> {
            rows.iter()
                .filter(|r| wanted.contains(&r.relation))
                .copied()
                .collect()
        };

        // The key's own Oxford entries.
        let mut headword_entries: Vec<(ExtEntry, Content)> = Vec::new();
        for row in of(&[Relation::Headword]) {
            headword_entries.push(self.oxford_pair(ext, row.entry_id)?);
        }

        // Base forms. With an Oxford entry of its own, a key that is also a variant of
        // another entry is just that entry's word (`break`, `autumn`), unless its own entry
        // says "= <that entry>" (`gaol`).
        let mut base: Vec<Hit> = Vec::new();
        let mut form_rows: Vec<&KeyRow> = Vec::new();
        for row in of(&[
            Relation::Inflection,
            Relation::Variant,
            Relation::Derivative,
        ]) {
            let content = self.oxford(ext, row.entry_id)?;
            if row.relation == Relation::Variant
                && !headword_entries.is_empty()
                && !headword_entries
                    .iter()
                    .any(|(e, _)| e.cross_refers_to(content.word()))
            {
                continue;
            }
            form_rows.push(row);
            base.push(Hit {
                relation: Some(describe(row, content.word())),
                content,
                role: Role::Lemma,
                open_group: row.group.clone(),
            });
        }
        let form_relation = base.first().and_then(|h| h.relation.clone());
        let own_role = if form_relation.is_some() {
            Role::FormOf
        } else {
            Role::Query
        };

        // A form note (`went`: "past tense of go") whose base form is shown is not an
        // entry of its own (6.3).
        let has_headword = !headword_entries.is_empty();
        let mut own: Vec<Hit> = Vec::new();
        for (entry, content) in headword_entries {
            if entry.is_form_note() && base.iter().any(|h| entry.is_form_note_of(h.content.word()))
            {
                continue;
            }
            own.push(Hit {
                content,
                role: own_role,
                relation: form_relation.clone(),
                open_group: None,
            });
        }
        sort_by_case(&mut own, query, options.sentence_start);

        // The key as a whole idiom or phrasal verb: shown even next to an entry of its own,
        // and itself an entry of the key's own (7.3), unless it only cross-refers.
        let mut whole: Vec<Hit> = Vec::new();
        let mut whole_written_as_query = false;
        let mut whole_is_own = false;
        for row in of(&[Relation::Idiom, Relation::Phrasal]) {
            let (entry, content) = self.oxford_pair(ext, row.entry_id)?;
            let group = row.group.as_deref().and_then(|g| entry.group(g));
            whole_is_own |= group.is_none_or(|g| !g.is_cross_reference_only());
            whole_written_as_query |= row
                .via
                .as_deref()
                .is_some_and(|v| written(v) == written(query));
            whole.push(Hit {
                relation: Some(describe(row, content.word())),
                content,
                role: Role::Lemma,
                open_group: row.group.clone(),
            });
        }

        if !has_headword && !whole_is_own {
            // ECDICT's own entry (not just "xx 的过去式"), with Oxford's form relation, or
            // else ECDICT's own (6.2 step 2).
            let relation = match &form_relation {
                Some(r) => Some(r.clone()),
                None => self.ecdict.exchange_lemma(query)?.map(|i| i.describe_zh()),
            };
            for entry in self.ecdict.entries_for(query, options.sentence_start)? {
                if !is_form_note(&entry) {
                    own.push(Hit {
                        content: Content::Ecdict(entry),
                        role: if relation.is_some() {
                            Role::FormOf
                        } else {
                            Role::Query
                        },
                        relation: relation.clone(),
                        open_group: None,
                    });
                }
            }
        }

        // An entry of the key's own hides the idioms and phrasal verbs it is part of (6.3).
        let mut parts: Vec<Hit> = Vec::new();
        if own.is_empty() && !whole_is_own {
            for row in of(&[Relation::IdiomPart, Relation::PhrasalPart]) {
                let content = self.oxford(ext, row.entry_id)?;
                parts.push(Hit {
                    relation: Some(describe(row, content.word())),
                    content,
                    role: Role::Lemma,
                    open_group: row.group.clone(),
                });
            }
        }

        let lemma = match form_rows.first() {
            Some(row) => match (row.relation, &row.via) {
                (Relation::Inflection | Relation::Derivative, Some(via)) => via.clone(),
                _ => base[0].content.word().to_owned(),
            },
            None => own
                .first()
                .map_or_else(|| query.to_owned(), |h| h.content.word().to_owned()),
        };
        // Both an entry of its own and a whole phrase (`all in`: the idiom, and `all-in`):
        // the one written as selected first (spaces and hyphens count, case does not), else
        // the entry.
        if whole_written_as_query
            && !own
                .iter()
                .any(|h| written(h.content.word()) == written(query))
        {
            whole.append(&mut own);
            own = whole;
        } else {
            own.extend(whole);
        }
        let own_first = own.iter().any(|h| h.content.is_marked());
        let others = base.into_iter().chain(parts);
        let hits: Vec<Hit> = if own_first {
            own.into_iter().chain(others).collect()
        } else {
            others.chain(own).collect()
        };
        if hits.is_empty() {
            return Ok(None);
        }
        Ok(Some(Merged {
            query: query.to_owned(),
            hits,
            lemma,
        }))
    }

    /// [`Lexicon::oxford`], keeping a copy of the bare entry for the rules that look into it.
    fn oxford_pair(&self, ext: &ExtDictionary, id: i64) -> Result<(ExtEntry, Content), DictError> {
        let content = self.oxford(ext, id)?;
        let Content::Oxford(o) = &content else {
            unreachable!("an Oxford entry");
        };
        Ok((o.entry.clone(), content))
    }

    /// An Oxford entry with ECDICT's additions (6.4).
    fn oxford(&self, ext: &ExtDictionary, id: i64) -> Result<Content, DictError> {
        let entry = ext.entry(id)?;
        let ecdict = self.ecdict.entry(&entry.headword)?;
        let no_phonetic = entry.phon_gb.is_none() && entry.phon_us.is_none();
        Ok(Content::Oxford(Box::new(OxfordEntry {
            tags: ecdict.as_ref().map(|e| e.tags.clone()).unwrap_or_default(),
            collins: ecdict.as_ref().and_then(|e| e.collins),
            ecdict_oxford: ecdict.as_ref().is_some_and(|e| e.oxford),
            ecdict_phonetic: ecdict.and_then(|e| e.phonetic).filter(|_| no_phonetic),
            entry,
        })))
    }

    /// An ECDICT result (6.2 step 3) with its base form's entry taken from Oxford when
    /// Oxford has that headword, or has it as a whole idiom or phrasal verb (`gave up`).
    fn with_oxford_lemma(
        &self,
        ext: &ExtDictionary,
        found: LookupResult,
    ) -> Result<Merged, DictError> {
        let mut merged = from_ecdict(found);
        let mut hits = Vec::new();
        for hit in std::mem::take(&mut merged.hits) {
            let ids = match hit.role {
                Role::Lemma => ext.lemma_entries(hit.content.word())?,
                _ => Vec::new(),
            };
            if ids.is_empty() {
                hits.push(hit);
                continue;
            }
            for (id, group) in ids {
                hits.push(Hit {
                    content: self.oxford(ext, id)?,
                    open_group: group,
                    ..hit.clone()
                });
            }
        }
        merged.hits = hits;
        Ok(merged)
    }
}

/// Oxford rows of a key; a hyphenated key Oxford lacks is tried with spaces and without
/// hyphens (`state-of-the-art`, `multi-task`, spec 6.2).
fn oxford_rows(ext: &ExtDictionary, query: &str) -> Result<Vec<KeyRow>, DictError> {
    let rows = ext.rows(query)?;
    if !rows.is_empty() || !query.contains('-') {
        return Ok(rows);
    }
    let rows = ext.rows(&query.replace('-', " "))?;
    if !rows.is_empty() {
        return Ok(rows);
    }
    ext.rows(&query.replace('-', ""))
}

/// A headword, idiom or phrasal verb as written, for comparing with the selected text:
/// lowercase, without stress marks, syllable dots and punctuation other than hyphens
/// (`all ˈin`, `big deal!`); `↔` is a space.
fn written(text: &str) -> String {
    let text: String = text
        .chars()
        .filter_map(|c| match c {
            '↔' => Some(' '),
            // Stress marks are modifier letters.
            'ˈ' | 'ˌ' | '·' => None,
            c if c.is_alphanumeric() || c == '-' || c.is_whitespace() => Some(c),
            _ => None,
        })
        .flat_map(char::to_lowercase)
        .collect();
    text.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// Chinese names of Oxford's kinds of form (`keys.kinds`): ECDICT's codes, then `neg`,
/// `short`, `sg`, `fem`.
fn kind_name(code: &str) -> Option<&'static str> {
    if let Some(kind) = FormKind::from_code(code) {
        return Some(kind.name_zh());
    }
    Some(match code {
        "neg" => "否定式",
        "short" => "缩略式",
        "sg" => "单数",
        "fem" => "阴性形式",
        _ => return None,
    })
}

/// The relation of a key to the entry `headword` of its row, for display (spec 6.1).
fn describe(row: &KeyRow, headword: &str) -> String {
    match row.relation {
        Relation::Inflection => {
            let base = row.via.as_deref().unwrap_or(headword);
            let mut names: Vec<&str> = Vec::new();
            for name in row.kinds.iter().filter_map(|k| kind_name(k)) {
                if !names.contains(&name) {
                    names.push(name);
                }
            }
            if names.is_empty() {
                format!("{base} 的变化形式")
            } else {
                format!("{base} 的{}", names.join("、"))
            }
        }
        Relation::Variant => format!("{headword} 的另一种写法"),
        Relation::Derivative => format!("{headword} 的派生词"),
        Relation::Idiom | Relation::IdiomPart => format!("{headword} 词条中的习语"),
        Relation::Phrasal | Relation::PhrasalPart => format!("{headword} 词条中的短语动词"),
        Relation::Headword | Relation::Other => String::new(),
    }
}

/// The text's own entries in the selected case first; at a sentence start a capitalized
/// word's lowercase entry first (`m1-dict.md` 8, spec 6.3). Otherwise key order.
fn sort_by_case(hits: &mut [Hit], query: &str, sentence_start: bool) {
    let lower = query.to_lowercase();
    let lowercase_first = sentence_start && is_capitalized(query);
    hits.sort_by_key(|h| {
        let word = h.content.word();
        let (first, second) = if lowercase_first {
            (word == lower, word == query)
        } else {
            (word == query, word == lower)
        };
        match (first, second) {
            (true, _) => 0,
            (_, true) => 1,
            _ => 2,
        }
    });
}

/// An ECDICT-only result in merged form (M1).
fn from_ecdict(found: LookupResult) -> Merged {
    let lemma = found.lemma().to_owned();
    let base = found.inflection.as_ref().map(|i| i.describe_zh());
    let hits = found
        .entries
        .into_iter()
        .map(|f| {
            let (role, relation) = match f.role {
                EntryRole::Query => (Role::Query, None),
                EntryRole::FormOf(i) => (Role::FormOf, Some(i.describe_zh())),
                EntryRole::Lemma => (Role::Lemma, base.clone()),
            };
            Hit {
                content: Content::Ecdict(f.entry),
                role,
                relation,
                open_group: None,
            }
        })
        .collect();
    Merged {
        query: found.query,
        hits,
        lemma,
    }
}

#[cfg(test)]
mod tests;
