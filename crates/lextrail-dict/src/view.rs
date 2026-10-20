//! A lookup shaped for display (`docs/specs/m1-popup-dict.md`): what the popup shows, and
//! later the PWA. Senses are grouped into one line per part of speech; domain senses
//! (`[计]`, `[医]`) are kept apart so the page can fold them.

use lextrail_core::lookup::LookupKind;
use serde::Serialize;

use crate::{
    Classification, DictError, Dictionary, Entry, EntryRole, Found, FoundEntry, LookupOptions,
    LookupResult, PartsResult, Sense,
};

/// How many part-of-speech lines each entry shows when a lookup gives several entries
/// (`better` and `good`); the rest open with "▸ 更多".
pub const LINES_WHEN_SEVERAL: usize = 3;

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum LookupView {
    /// The text is in the dictionary, possibly through its base form.
    Entries {
        /// Shown above a base form's entry when the text has no entry of its own:
        /// "went 是 go 的过去式".
        note: Option<String>,
        entries: Vec<EntryView>,
    },
    /// A hyphenated compound the dictionary does not have, split into parts.
    Parts {
        parts: Vec<PartView>,
        not_found: Vec<String>,
    },
    NotFound,
    /// Several words that are not a dictionary phrase: translated by the AI (M2).
    Sentence,
    /// No English letters (D045).
    NotEnglish,
}

impl LookupView {
    /// The kind of result, as serialized (`"entries"`, `"not_found"`, ...): for logs that
    /// must not show the looked-up text.
    pub fn kind(&self) -> &'static str {
        match self {
            Self::Entries { .. } => "entries",
            Self::Parts { .. } => "parts",
            Self::NotFound => "not_found",
            Self::Sentence => "sentence",
            Self::NotEnglish => "not_english",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct EntryView {
    pub word: String,
    /// "good 的比较级" for a form that also has an entry of its own (`better`).
    pub relation: Option<String>,
    pub phonetic: Option<String>,
    /// Exam tags in Chinese ("四级", "GRE"), then "牛津" for the Oxford 3000.
    pub labels: Vec<String>,
    /// Collins stars, 0 for none.
    pub collins: u8,
    /// Senses without a domain label, one line per part of speech.
    pub general: Vec<SenseLine>,
    /// Senses with a domain label, folded under "▸ 专业释义（N）".
    pub domain: Vec<SenseLine>,
    /// Domain senses start unfolded: the entry has nothing else to show.
    pub domain_open: bool,
    /// How many `general` lines are shown before "▸ 更多"; `None` for all.
    pub max_lines: Option<usize>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct SenseLine {
    pub pos: Option<String>,
    pub domain: Option<String>,
    /// The senses of this line joined with "；".
    pub text: String,
}

/// One part of a compound: its first line only.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct PartView {
    /// The part as written ("models").
    pub word: String,
    /// "model 的复数" when the part is a form of another word.
    pub relation: Option<String>,
    pub phonetic: Option<String>,
    pub line: Option<SenseLine>,
}

/// A lookup for display, with what the lookup history stores about it
/// (`ARCHITECTURE.md` 6.3).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Looked {
    pub view: LookupView,
    /// `None` for text without English letters, which is not recorded (D045).
    pub kind: Option<LookupKind>,
    /// The base form (`lookups.lemma`, [`LookupResult::lemma`]); `None` for sentences,
    /// text not found and compounds found only as parts.
    pub lemma: Option<String>,
}

impl Dictionary {
    /// Classifies `text` (D045) and, for a word or phrase, looks it up (splitting a
    /// compound the dictionary lacks) and shapes the result for display. `options`: see
    /// [`LookupOptions`].
    pub fn view(&self, text: &str, options: LookupOptions) -> Result<LookupView, DictError> {
        Ok(self.look_up(text, options)?.view)
    }

    /// Like [`Dictionary::view`], with the classification and base form for the lookup
    /// history.
    pub fn look_up(&self, text: &str, options: LookupOptions) -> Result<Looked, DictError> {
        let kind = match self.classify(text)? {
            Classification::NotEnglish => {
                return Ok(Looked {
                    view: LookupView::NotEnglish,
                    kind: None,
                    lemma: None,
                });
            }
            Classification::English(kind) => kind,
        };
        let (view, lemma) = match kind {
            LookupKind::Sentence => (LookupView::Sentence, None),
            LookupKind::Word | LookupKind::Phrase => {
                match self.lookup_with_parts(text, options)? {
                    None => (LookupView::NotFound, None),
                    Some(Found::Whole(result)) => {
                        (entries_view(&result), Some(result.lemma().to_owned()))
                    }
                    Some(Found::Parts(parts)) => (parts_view(&parts), None),
                }
            }
        };
        Ok(Looked {
            view,
            kind: Some(kind),
            lemma,
        })
    }
}

fn entries_view(result: &LookupResult) -> LookupView {
    let note = match (result.entries.as_slice(), &result.inflection) {
        ([only], Some(i)) if only.role == EntryRole::Lemma => {
            Some(format!("{} 是 {}", i.form, i.describe_zh()))
        }
        _ => None,
    };
    let several = result.entries.len() > 1;
    let entries = result
        .entries
        .iter()
        .map(|found| {
            let (general, domain) = sense_lines(&found.entry.senses);
            EntryView {
                word: found.entry.word.clone(),
                relation: relation(found),
                phonetic: found.entry.phonetic.clone(),
                labels: labels(&found.entry),
                collins: found.entry.collins.unwrap_or(0),
                domain_open: general.is_empty() && !domain.is_empty(),
                max_lines: several.then_some(LINES_WHEN_SEVERAL),
                general,
                domain,
            }
        })
        .collect();
    LookupView::Entries { note, entries }
}

fn parts_view(compound: &PartsResult) -> LookupView {
    let parts = compound
        .parts
        .iter()
        .map(|result| {
            let found = &result.entries[0];
            let (general, domain) = sense_lines(&found.entry.senses);
            // A part found through its base form ("models" -> model) is shown as written.
            let (word, relation) = match (&found.role, &result.inflection) {
                (EntryRole::Lemma, Some(i)) => (i.form.clone(), Some(i.describe_zh())),
                _ => (found.entry.word.clone(), relation(found)),
            };
            PartView {
                word,
                relation,
                phonetic: found.entry.phonetic.clone(),
                line: general.into_iter().chain(domain).next(),
            }
        })
        .collect();
    LookupView::Parts {
        parts,
        not_found: compound.not_found.clone(),
    }
}

fn relation(found: &FoundEntry) -> Option<String> {
    match &found.role {
        EntryRole::FormOf(i) => Some(i.describe_zh()),
        EntryRole::Query | EntryRole::Lemma => None,
    }
}

/// ECDICT exam tags in display order, with their Chinese names.
const TAGS: [(&str, &str); 8] = [
    ("zk", "中考"),
    ("gk", "高考"),
    ("cet4", "四级"),
    ("cet6", "六级"),
    ("ky", "考研"),
    ("toefl", "托福"),
    ("ielts", "雅思"),
    ("gre", "GRE"),
];

fn labels(entry: &Entry) -> Vec<String> {
    let mut labels: Vec<String> = TAGS
        .iter()
        .filter(|(code, _)| entry.tags.iter().any(|t| t == code))
        .map(|(_, name)| (*name).to_owned())
        .collect();
    if entry.oxford {
        labels.push("牛津".to_owned());
    }
    labels
}

/// Groups senses into lines in source order: neighbours with the same part of speech
/// (and domain) share a line. Returns (general, domain) lines.
fn sense_lines(senses: &[Sense]) -> (Vec<SenseLine>, Vec<SenseLine>) {
    let mut general: Vec<SenseLine> = Vec::new();
    let mut domain: Vec<SenseLine> = Vec::new();
    // Which list the previous sense went to, so only neighbours are joined.
    let mut previous_was_domain = None;
    for sense in senses {
        let is_domain = sense.domain.is_some();
        let lines = if is_domain { &mut domain } else { &mut general };
        match lines.last_mut() {
            Some(last)
                if previous_was_domain == Some(is_domain)
                    && last.pos == sense.pos
                    && last.domain == sense.domain =>
            {
                last.text.push('；');
                last.text.push_str(&sense.text);
            }
            _ => lines.push(SenseLine {
                pos: sense.pos.clone(),
                domain: sense.domain.clone(),
                text: sense.text.clone(),
            }),
        }
        previous_was_domain = Some(is_domain);
    }
    (general, domain)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Inflection, InflectionSource};

    fn sense(pos: Option<&str>, domain: Option<&str>, text: &str) -> Sense {
        Sense {
            pos: pos.map(str::to_owned),
            domain: domain.map(str::to_owned),
            text: text.to_owned(),
        }
    }

    fn entry(word: &str, senses: Vec<Sense>) -> Entry {
        Entry {
            word: word.to_owned(),
            phonetic: None,
            tags: vec![],
            collins: None,
            oxford: false,
            bnc: None,
            frq: None,
            senses,
        }
    }

    fn line(pos: Option<&str>, domain: Option<&str>, text: &str) -> SenseLine {
        SenseLine {
            pos: pos.map(str::to_owned),
            domain: domain.map(str::to_owned),
            text: text.to_owned(),
        }
    }

    #[test]
    fn senses_are_grouped_by_neighbouring_part_of_speech() {
        let senses = vec![
            sense(Some("n."), None, "下水"),
            sense(Some("n."), None, "汽艇"),
            sense(Some("vt."), None, "发射"),
            sense(Some("n."), Some("计"), "启动"),
            sense(Some("n."), Some("计"), "开办"),
            sense(Some("n."), None, "发行"),
            sense(None, Some("医"), "原"),
        ];
        let (general, domain) = sense_lines(&senses);
        assert_eq!(
            general,
            [
                line(Some("n."), None, "下水；汽艇"),
                line(Some("vt."), None, "发射"),
                // Not joined with the first line: a domain line came between.
                line(Some("n."), None, "发行"),
            ]
        );
        assert_eq!(
            domain,
            [
                line(Some("n."), Some("计"), "启动；开办"),
                line(None, Some("医"), "原"),
            ]
        );
    }

    #[test]
    fn labels_in_chinese_in_a_fixed_order() {
        let mut e = entry("launch", vec![]);
        e.tags = vec!["gre".into(), "cet6".into(), "cet4".into(), "unknown".into()];
        e.oxford = true;
        assert_eq!(labels(&e), ["四级", "六级", "GRE", "牛津"]);
    }

    #[test]
    fn entry_with_only_domain_senses_starts_unfolded() {
        let result = LookupResult {
            query: "convolutional".into(),
            entries: vec![FoundEntry {
                entry: entry(
                    "convolutional",
                    vec![
                        sense(Some("a."), Some("计"), "卷积的"),
                        sense(Some("a."), Some("医"), "脑回的"),
                    ],
                ),
                role: EntryRole::Query,
            }],
            inflection: None,
        };
        let LookupView::Entries { note, entries } = entries_view(&result) else {
            panic!("entries");
        };
        assert_eq!(note, None);
        assert!(entries[0].general.is_empty());
        assert_eq!(entries[0].domain.len(), 2);
        assert!(entries[0].domain_open);
        assert_eq!(entries[0].max_lines, None);
    }

    #[test]
    fn forms_get_a_relation_or_a_note() {
        let launched = Inflection {
            form: "launched".into(),
            lemma: "launch".into(),
            kinds: vec![crate::FormKind::Past, crate::FormKind::PastParticiple],
            source: InflectionSource::Exchange,
        };
        // Only the base form's entry: a note above it.
        let result = LookupResult {
            query: "launched".into(),
            entries: vec![FoundEntry {
                entry: entry("launch", vec![sense(Some("n."), None, "发射")]),
                role: EntryRole::Lemma,
            }],
            inflection: Some(launched),
        };
        let LookupView::Entries { note, entries } = entries_view(&result) else {
            panic!("entries");
        };
        assert_eq!(
            note.as_deref(),
            Some("launched 是 launch 的过去式、过去分词")
        );
        assert_eq!(entries[0].relation, None);
        assert!(!entries[0].domain_open);

        // Both entries: the form's relation in its title; three lines each.
        let better = Inflection {
            form: "better".into(),
            lemma: "good".into(),
            kinds: vec![crate::FormKind::Comparative],
            source: InflectionSource::Exchange,
        };
        let result = LookupResult {
            query: "better".into(),
            entries: vec![
                FoundEntry {
                    entry: entry("better", vec![sense(Some("a."), None, "更好的")]),
                    role: EntryRole::FormOf(better.clone()),
                },
                FoundEntry {
                    entry: entry("good", vec![sense(Some("a."), None, "好的")]),
                    role: EntryRole::Lemma,
                },
            ],
            inflection: Some(better),
        };
        let LookupView::Entries { note, entries } = entries_view(&result) else {
            panic!("entries");
        };
        assert_eq!(note, None);
        assert_eq!(entries[0].relation.as_deref(), Some("good 的比较级"));
        assert_eq!(entries[1].relation, None);
        assert_eq!(entries[0].max_lines, Some(LINES_WHEN_SEVERAL));
    }

    #[test]
    fn serializes_with_kind_tag() {
        assert_eq!(
            serde_json::to_string(&LookupView::NotFound).unwrap(),
            r#"{"kind":"not_found"}"#
        );
        let parts = LookupView::Parts {
            parts: vec![PartView {
                word: "models".into(),
                relation: Some("model 的复数".into()),
                phonetic: None,
                line: Some(line(Some("n."), None, "模型")),
            }],
            not_found: vec!["qux".into()],
        };
        assert_eq!(
            serde_json::to_string(&parts).unwrap(),
            r#"{"kind":"parts","parts":[{"word":"models","relation":"model 的复数","phonetic":null,"line":{"pos":"n.","domain":null,"text":"模型"}}],"not_found":["qux"]}"#
        );
    }
}
