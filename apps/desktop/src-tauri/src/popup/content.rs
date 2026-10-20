//! What the popup page displays, sent to it as the `popup:content` event.

use lextrail_capture::{CaptureError, CaptureMethod, CaptureOutcome, Unavailable};
use lextrail_core::lookup::normalize_text;
use lextrail_core::text::{
    join_unmarked_line_breaks, resolve_line_end_hyphens, sentence_around, starts_sentence,
};
use lextrail_dict::view::{Looked, LookupView};
use lextrail_dict::{Classification, Dictionary, LookupOptions};
use std::fmt;

use serde::Serialize;

use crate::history::Record;

pub const EVENT: &str = "popup:content";

/// `Debug` never shows captured text (debug logs print requests and actions): a
/// selection shows only lengths and what kind of result it is.
#[derive(Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Content {
    /// Nothing; sent when the popup hides so a stale lookup never flashes up again.
    Empty,
    /// Capture is taking a while.
    Loading,
    /// The clipboard fallback waits for the hotkey to be released (held for a while).
    ReleaseKeys,
    Selection {
        text: String,
        sentence: Option<String>,
        /// The foreground program's process name, debug builds only: shown so a test can
        /// tell which window was captured. Release builds never send where a word came
        /// from (D046 supplement), and it is never stored.
        #[serde(skip_serializing_if = "Option::is_none")]
        source_app: Option<String>,
        /// `None` when the dictionary file is missing or unreadable.
        lookup: Option<LookupView>,
    },
    Unavailable {
        reason: &'static str,
    },
    Error {
        message: String,
    },
}

impl fmt::Debug for Content {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Empty => f.write_str("Empty"),
            Self::Loading => f.write_str("Loading"),
            Self::ReleaseKeys => f.write_str("ReleaseKeys"),
            Self::Selection {
                text,
                sentence,
                source_app,
                lookup,
            } => f
                .debug_struct("Selection")
                .field("text_chars", &text.chars().count())
                .field(
                    "sentence_chars",
                    &sentence.as_ref().map(|s| s.chars().count()),
                )
                .field("source_app", source_app)
                .field("lookup", &lookup.as_ref().map(LookupView::kind))
                .finish(),
            Self::Unavailable { reason } => f
                .debug_struct("Unavailable")
                .field("reason", reason)
                .finish(),
            // Error messages come from capture errors, never from the captured text.
            Self::Error { message } => f.debug_struct("Error").field("message", message).finish(),
        }
    }
}

impl Content {
    /// Builds the content for a capture result. With a dictionary, words broken across
    /// lines are joined first, in the selection and in its sentence (D044 supplement):
    /// PDFium's line-end hyphen marker everywhere; in text copied from a Chromium PDF
    /// viewer (`pdf_clipboard`, where the hyphen is gone) unmarked line breaks too.
    ///
    /// Also gives what the lookup history records: captured English text, as shown
    /// (`None` for anything else, D045). Without a dictionary it is classified on its own
    /// and has no base form.
    pub fn from_capture(
        result: Result<CaptureOutcome, CaptureError>,
        pdf_clipboard: bool,
        dict: Option<&Dictionary>,
    ) -> (Self, Option<Record>) {
        match result {
            Ok(CaptureOutcome::Captured(selection)) => {
                let raw_sentence = selection
                    .context
                    .as_ref()
                    .and_then(|c| sentence_around(&c.text, &selection.text, c.selection.clone()));
                let (text, sentence) = match dict {
                    Some(dict) => {
                        let is_word = |w: &str| dict.is_word(w).unwrap_or(false);
                        let mut text = resolve_line_end_hyphens(&selection.text, is_word);
                        if pdf_clipboard
                            && matches!(
                                selection.method,
                                CaptureMethod::Clipboard | CaptureMethod::ClipboardDoubleCopy
                            )
                        {
                            text = join_unmarked_line_breaks(&text, is_word);
                        }
                        let sentence = raw_sentence.map(|s| resolve_line_end_hyphens(s, is_word));
                        (text, sentence)
                    }
                    None => (selection.text.clone(), raw_sentence.map(str::to_owned)),
                };
                let text = normalize_text(&text);
                if text.is_empty() {
                    let content = Self::Unavailable {
                        reason: reason(Unavailable::NoSelection),
                    };
                    return (content, None);
                }
                let sentence = sentence
                    .map(|s| normalize_text(&s))
                    .filter(|s| !s.is_empty() && *s != text);
                let options = LookupOptions {
                    sentence_start: sentence.as_ref().is_some_and(|s| starts_sentence(s, &text)),
                };
                let looked = dict.and_then(|dict| match dict.look_up(&text, options) {
                    Ok(looked) => Some(looked),
                    Err(e) => {
                        eprintln!("dictionary lookup failed: {e}");
                        None
                    }
                });
                let (kind, lemma) = match &looked {
                    Some(l) => (l.kind, l.lemma.clone()),
                    None => match lextrail_dict::classify(&text, |_| false) {
                        Classification::English(kind) => (Some(kind), None),
                        Classification::NotEnglish => (None, None),
                    },
                };
                let record = kind.map(|kind| Record {
                    query: text.clone(),
                    kind,
                    lemma,
                    context: sentence.clone(),
                    method: selection.method,
                });
                let content = Self::Selection {
                    sentence,
                    text,
                    source_app: None,
                    lookup: looked.map(|Looked { view, .. }| view),
                };
                (content, record)
            }
            Ok(CaptureOutcome::Unavailable(u)) => (Self::Unavailable { reason: reason(u) }, None),
            Err(e) => (
                Self::Error {
                    message: e.to_string(),
                },
                None,
            ),
        }
    }

    /// With double Ctrl+C switched on, the "run as administrator" hint no longer says to
    /// switch it on first (D012 supplement).
    pub fn with_double_copy_on(self, on: bool) -> Self {
        match self {
            Self::Unavailable { reason: "elevated" } if on => Self::Unavailable {
                reason: "elevated_double_copy",
            },
            other => other,
        }
    }

    /// Debug builds: adds the foreground program's process name to a selection.
    #[cfg(debug_assertions)]
    pub fn with_source_app(mut self, app: Option<String>) -> Self {
        if let Self::Selection { source_app, .. } = &mut self {
            *source_app = app;
        }
        self
    }
}

/// Whether a double Ctrl+C on `text` is looked up (D012 supplement): text with a space
/// (a phrase or a sentence), or a single word the dictionary has (as an entry, or a
/// hyphenated compound whose parts it all has). Other single tokens (passwords, keys,
/// user names, e-mail and web addresses, order numbers) are quietly ignored, and so is
/// a single word when there is no dictionary to check it against. `Err` says why,
/// never with the text. The hotkey has no such rule.
pub fn double_copy_wanted(text: &str, dict: Option<&Dictionary>) -> Result<(), &'static str> {
    let text = normalize_text(text);
    if text.contains(' ') {
        return Ok(());
    }
    let dict = dict.ok_or("no dictionary to check a single word")?;
    match dict.look_up(&text, LookupOptions::default()) {
        Ok(Looked {
            view: LookupView::Entries { .. },
            ..
        }) => Ok(()),
        Ok(Looked {
            view: LookupView::Parts { not_found, .. },
            ..
        }) if not_found.is_empty() => Ok(()),
        Ok(_) => Err("single word not in the dictionary"),
        Err(_) => Err("dictionary lookup failed"),
    }
}

fn reason(u: Unavailable) -> &'static str {
    match u {
        Unavailable::NoSelection => "no_selection",
        Unavailable::Elevated => "elevated",
        Unavailable::NoForegroundWindow => "no_foreground_window",
        Unavailable::Unsupported => "unsupported",
        Unavailable::TimedOut => "timed_out",
        Unavailable::KeysHeld => "keys_held",
    }
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use lextrail_capture::{RawContext, Selection};
    use lextrail_dict::FORMAT_VERSION;
    use lextrail_dict::schema::{INDEXES, SCHEMA};
    use rusqlite::{Connection, params};

    use super::*;

    /// A dictionary with a few entries (word, pos, sense) and a full-ECDICT headword.
    fn dictionary(dir: &Path) -> Dictionary {
        let path = dir.join("dict.db");
        let conn = Connection::open(&path).unwrap();
        conn.execute_batch(SCHEMA).unwrap();
        conn.execute(
            "INSERT INTO meta (key, value) VALUES ('format_version', ?1)",
            [FORMAT_VERSION.to_string()],
        )
        .unwrap();
        let entries = [
            ("address", "vt.", "处理"),
            ("transduction", "n.", "转导"),
            ("the", "art.", "这"),
            ("The", "", "(人名) 西"),
            ("model", "n.", "模型"),
            ("sequence", "n.", "序列"),
            ("aligned", "a.", "对齐的"),
        ];
        for (id, (word, pos, text)) in entries.iter().enumerate() {
            let id = id as i64 + 1;
            conn.execute(
                "INSERT INTO entries (id, word) VALUES (?1, ?2)",
                params![id, word],
            )
            .unwrap();
            conn.execute(
                "INSERT INTO senses (entry_id, ord, pos, text) VALUES (?1, 0, ?2, ?3)",
                params![id, Some(*pos).filter(|p| !p.is_empty()), text],
            )
            .unwrap();
        }
        conn.execute(
            "INSERT INTO extra_words (word) VALUES ('convolutional')",
            [],
        )
        .unwrap();
        conn.execute_batch(INDEXES).unwrap();
        drop(conn);
        Dictionary::open(&path).unwrap()
    }

    /// The content only.
    fn build(
        result: Result<CaptureOutcome, CaptureError>,
        pdf_clipboard: bool,
        dict: Option<&Dictionary>,
    ) -> Content {
        Content::from_capture(result, pdf_clipboard, dict).0
    }

    fn captured(
        text: &str,
        context: Option<RawContext>,
        method: CaptureMethod,
    ) -> Result<CaptureOutcome, CaptureError> {
        Ok(CaptureOutcome::Captured(Selection {
            text: text.into(),
            context,
            method,
        }))
    }

    fn uia(text: &str, context: Option<RawContext>) -> Result<CaptureOutcome, CaptureError> {
        captured(text, context, CaptureMethod::Uia)
    }

    /// `paragraph` as UIA context, with the selection at the first `selected`.
    fn context(paragraph: &str, selected: &str) -> RawContext {
        let at = paragraph.find(selected).unwrap();
        RawContext {
            text: paragraph.into(),
            selection: Some(at..at + selected.len()),
        }
    }

    fn words(content: &Content) -> Vec<String> {
        match content {
            Content::Selection {
                lookup: Some(LookupView::Entries { entries, .. }),
                ..
            } => entries.iter().map(|e| e.word.clone()).collect(),
            other => panic!("{other:?}"),
        }
    }

    fn text_and_sentence(content: &Content) -> (&str, Option<&str>) {
        match content {
            Content::Selection { text, sentence, .. } => (text, sentence.as_deref()),
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn selection_with_sentence_and_entry() {
        let dir = tempfile::tempdir().unwrap();
        let dict = dictionary(dir.path());
        let context = RawContext {
            text: "First. We must  address it. Last.".into(),
            selection: Some(16..23),
        };
        let content = build(uia(" address ", Some(context)), false, Some(&dict));
        assert_eq!(
            text_and_sentence(&content),
            ("address", Some("We must address it."))
        );
        assert_eq!(words(&content), ["address"]);
        // Where it came from is not part of the content (debug builds add it later).
        assert!(matches!(
            content,
            Content::Selection {
                source_app: None,
                ..
            }
        ));
    }

    #[test]
    fn line_end_hyphens_are_joined_in_selection_and_sentence() {
        let dir = tempfile::tempdir().unwrap();
        let dict = dictionary(dir.path());
        // As Chrome's PDF viewer reports them through UIA (R5).
        let paragraph = "Sequence modeling and transduc\u{2}tion models in various tasks. \
                         Instead of sequence\u{2}aligned recurrence.";
        let selected = "transduc\u{2}tion";
        let content = build(
            uia(&format!(" {selected} "), Some(context(paragraph, selected))),
            false,
            Some(&dict),
        );
        assert_eq!(
            text_and_sentence(&content),
            (
                "transduction",
                Some("Sequence modeling and transduction models in various tasks.")
            )
        );
        assert_eq!(words(&content), ["transduction"]);

        // A compound's hyphen is kept, in the sentence too, and it is split into parts.
        let selected = "sequence\u{2}aligned";
        let content = build(
            uia(selected, Some(context(paragraph, selected))),
            false,
            Some(&dict),
        );
        assert_eq!(
            text_and_sentence(&content),
            (
                "sequence-aligned",
                Some("Instead of sequence-aligned recurrence.")
            )
        );
        assert!(matches!(
            content,
            Content::Selection {
                lookup: Some(LookupView::Parts { .. }),
                ..
            }
        ));
    }

    #[test]
    fn pdf_clipboard_line_breaks_are_joined_only_for_word_fragments() {
        let dir = tempfile::tempdir().unwrap();
        let dict = dictionary(dir.path());
        let clipboard = |text: &str, pdf: bool| {
            let content = build(
                captured(text, None, CaptureMethod::Clipboard),
                pdf,
                Some(&dict),
            );
            text_and_sentence(&content).0.to_owned()
        };
        // As Edge's PDF viewer copies them (R5): the hyphen is gone.
        assert_eq!(clipboard(" transduc\r\ntion m", true), "transduction m");
        assert_eq!(clipboard("convolu\r\ntional", true), "convolutional");
        assert_eq!(clipboard("sequence\r\naligned", true), "sequence aligned");
        // Other clipboard text keeps its line breaks (as spaces).
        assert_eq!(clipboard("transduc\r\ntion", false), "transduc tion");
        // A double Ctrl+C in the PDF viewer copies the same way.
        let double = build(
            captured(
                "convolu\r\ntional",
                None,
                CaptureMethod::ClipboardDoubleCopy,
            ),
            true,
            Some(&dict),
        );
        assert_eq!(text_and_sentence(&double).0, "convolutional");
    }

    #[test]
    fn case_order_follows_the_selection_and_the_sentence_start() {
        let dir = tempfile::tempdir().unwrap();
        let dict = dictionary(dir.path());
        let in_paragraph = |paragraph: &str| {
            build(
                uia("The", Some(context(paragraph, "The"))),
                false,
                Some(&dict),
            )
        };
        // No sentence: the selected text's own case first.
        let content = build(uia("The", None), false, Some(&dict));
        assert_eq!(words(&content), ["The", "the"]);
        // At a sentence start: lowercase first.
        assert_eq!(words(&in_paragraph("The model is large.")), ["the", "The"]);
        // Mid-sentence: its own case first.
        assert_eq!(words(&in_paragraph("We met The model.")), ["The", "the"]);
    }

    #[test]
    fn without_a_dictionary_the_text_is_still_shown() {
        let content = build(uia("transduc\u{2}tion", None), false, None);
        assert_eq!(
            content,
            Content::Selection {
                text: "transduc-tion".into(),
                sentence: None,
                source_app: None,
                lookup: None,
            }
        );
    }

    #[test]
    fn history_record_is_what_the_popup_shows() {
        use lextrail_core::lookup::LookupKind;
        let dir = tempfile::tempdir().unwrap();
        let dict = dictionary(dir.path());
        let paragraph = "We use sequence models. Then more.";
        let (_, record) = Content::from_capture(
            uia(" models ", Some(context(paragraph, "models"))),
            false,
            Some(&dict),
        );
        assert_eq!(
            record,
            Some(Record {
                query: "models".into(),
                kind: LookupKind::Word,
                lemma: Some("model".into()),
                context: Some("We use sequence models.".into()),
                method: CaptureMethod::Uia,
            })
        );
        // The joined word, as shown; nothing found: no base form.
        let (_, record) = Content::from_capture(
            captured("convolu\r\ntional", None, CaptureMethod::Clipboard),
            true,
            Some(&dict),
        );
        let record = record.unwrap();
        assert_eq!(
            (record.query.as_str(), record.lemma, record.method),
            ("convolutional", None, CaptureMethod::Clipboard)
        );
        // Without a dictionary: classified on its own.
        let (_, record) = Content::from_capture(uia("We give up on it.", None), false, None);
        let record = record.unwrap();
        assert_eq!((record.kind, record.lemma), (LookupKind::Sentence, None));
        // Not recorded: no English (D045), nothing captured.
        for result in [
            uia("注意力", None),
            uia(" ", None),
            Ok(CaptureOutcome::Unavailable(Unavailable::NoSelection)),
        ] {
            assert_eq!(Content::from_capture(result, false, Some(&dict)).1, None);
        }
    }

    #[test]
    fn sentence_equal_to_selection_is_dropped() {
        let context = RawContext {
            text: "Hello world.".into(),
            selection: None,
        };
        let content = build(uia("Hello world.", Some(context)), false, None);
        assert!(matches!(content, Content::Selection { sentence: None, .. }));
    }

    #[test]
    fn whitespace_only_selection_is_no_selection() {
        assert_eq!(
            build(uia(" \n ", None), false, None),
            Content::Unavailable {
                reason: "no_selection"
            }
        );
    }

    #[test]
    fn unavailable_and_error() {
        let unavailable = |u| build(Ok(CaptureOutcome::Unavailable(u)), false, None);
        assert_eq!(
            unavailable(Unavailable::Elevated),
            Content::Unavailable { reason: "elevated" }
        );
        assert_eq!(
            unavailable(Unavailable::KeysHeld),
            Content::Unavailable {
                reason: "keys_held"
            }
        );
        let err = CaptureError::WorkerUnavailable("gone".into());
        assert!(matches!(
            build(Err(err), false, None),
            Content::Error { message } if message.contains("gone")
        ));
    }

    #[test]
    fn double_copy_looks_up_dictionary_words_and_text_with_spaces() {
        let dir = tempfile::tempdir().unwrap();
        let dict = dictionary(dir.path());
        for wanted in [
            "address",
            " model\r\n",
            "models",
            "sequence-aligned",
            "We must address it.",
            "the model",
            "two unknownwords",
        ] {
            assert_eq!(
                double_copy_wanted(wanted, Some(&dict)),
                Ok(()),
                "{wanted:?}"
            );
        }
        for ignored in [
            "Hunter2-Secret",
            "xK9#pL2$vQ",
            "sk-abc123def456",
            "someone@example.com",
            "https://example.com/a?b=c",
            "user_name",
            "ORD-2026-000123",
            "qwzxv",
            "convolutional",
            "注意力",
        ] {
            assert!(
                double_copy_wanted(ignored, Some(&dict)).is_err(),
                "{ignored:?}"
            );
        }
        // Without a dictionary a single word cannot be checked: ignored.
        assert!(double_copy_wanted("address", None).is_err());
        assert_eq!(double_copy_wanted("We must address it.", None), Ok(()));
    }

    #[test]
    fn elevated_hint_follows_the_double_copy_setting() {
        let elevated = Content::Unavailable { reason: "elevated" };
        assert_eq!(
            elevated.clone().with_double_copy_on(true),
            Content::Unavailable {
                reason: "elevated_double_copy"
            }
        );
        assert_eq!(elevated.clone().with_double_copy_on(false), elevated);
        let other = Content::Unavailable {
            reason: "no_selection",
        };
        assert_eq!(other.clone().with_double_copy_on(true), other);
    }

    #[test]
    fn debug_output_never_shows_the_text() {
        let content = Content::Selection {
            text: "hunter2secret".into(),
            sentence: Some("my hunter2secret here".into()),
            source_app: None,
            lookup: Some(LookupView::NotFound),
        };
        let debug = format!("{content:?}");
        assert!(!debug.contains("hunter2"), "{debug}");
        assert!(
            debug.contains("text_chars: 13") && debug.contains("not_found"),
            "{debug}"
        );
    }

    #[test]
    fn serializes_with_kind_tag() {
        let json = serde_json::to_string(&Content::Unavailable { reason: "elevated" }).unwrap();
        assert_eq!(json, r#"{"kind":"unavailable","reason":"elevated"}"#);
        assert_eq!(
            serde_json::to_string(&Content::Loading).unwrap(),
            r#"{"kind":"loading"}"#
        );
        assert_eq!(
            serde_json::to_string(&Content::ReleaseKeys).unwrap(),
            r#"{"kind":"release_keys"}"#
        );
        let selection = Content::Selection {
            text: "qux".into(),
            sentence: None,
            source_app: None,
            lookup: Some(LookupView::NotFound),
        };
        // No source field at all unless a debug build adds one.
        assert_eq!(
            serde_json::to_string(&selection).unwrap(),
            r#"{"kind":"selection","text":"qux","sentence":null,"lookup":{"kind":"not_found"}}"#
        );
        #[cfg(debug_assertions)]
        assert!(
            serde_json::to_string(&selection.with_source_app(Some("chrome.exe".into())))
                .unwrap()
                .contains(r#""source_app":"chrome.exe""#)
        );
    }
}
