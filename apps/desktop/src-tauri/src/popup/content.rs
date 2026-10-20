//! What the popup page displays, sent to it as the `popup:content` event.

use lextrail_capture::{CaptureError, CaptureMethod, CaptureOutcome, Unavailable};
use lextrail_core::lookup::normalize_text;
use lextrail_core::text::{
    join_unmarked_line_breaks, resolve_line_end_hyphens, sentence_around, starts_sentence,
};
use lextrail_dict::view::LookupView;
use lextrail_dict::{Dictionary, LookupOptions};
use serde::Serialize;

pub const EVENT: &str = "popup:content";

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
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

impl Content {
    /// Builds the content for a capture result. With a dictionary, words broken across
    /// lines are joined first, in the selection and in its sentence (D044 supplement):
    /// PDFium's line-end hyphen marker everywhere; in text copied from a Chromium PDF
    /// viewer (`pdf_clipboard`, where the hyphen is gone) unmarked line breaks too.
    pub fn from_capture(
        result: Result<CaptureOutcome, CaptureError>,
        pdf_clipboard: bool,
        source_app: Option<String>,
        dict: Option<&Dictionary>,
    ) -> Self {
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
                        if pdf_clipboard && selection.method == CaptureMethod::Clipboard {
                            text = join_unmarked_line_breaks(&text, is_word);
                        }
                        let sentence = raw_sentence.map(|s| resolve_line_end_hyphens(s, is_word));
                        (text, sentence)
                    }
                    None => (selection.text.clone(), raw_sentence.map(str::to_owned)),
                };
                let text = normalize_text(&text);
                if text.is_empty() {
                    return Self::Unavailable {
                        reason: reason(Unavailable::NoSelection),
                    };
                }
                let sentence = sentence.map(|s| normalize_text(&s));
                let options = LookupOptions {
                    sentence_start: sentence.as_ref().is_some_and(|s| starts_sentence(s, &text)),
                };
                let lookup = dict.and_then(|dict| match dict.view(&text, options) {
                    Ok(view) => Some(view),
                    Err(e) => {
                        eprintln!("dictionary lookup failed: {e}");
                        None
                    }
                });
                Self::Selection {
                    sentence: sentence.filter(|s| !s.is_empty() && *s != text),
                    text,
                    source_app,
                    lookup,
                }
            }
            Ok(CaptureOutcome::Unavailable(u)) => Self::Unavailable { reason: reason(u) },
            Err(e) => Self::Error {
                message: e.to_string(),
            },
        }
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

    fn captured(
        text: &str,
        context: Option<RawContext>,
        method: CaptureMethod,
    ) -> Result<CaptureOutcome, CaptureError> {
        Ok(CaptureOutcome::Captured(Selection {
            text: text.into(),
            context,
            url: None,
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
        let content = Content::from_capture(
            uia(" address ", Some(context)),
            false,
            Some("chrome.exe".into()),
            Some(&dict),
        );
        assert_eq!(
            text_and_sentence(&content),
            ("address", Some("We must address it."))
        );
        assert_eq!(words(&content), ["address"]);
        assert!(matches!(
            content,
            Content::Selection { source_app: Some(ref app), .. } if app == "chrome.exe"
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
        let content = Content::from_capture(
            uia(&format!(" {selected} "), Some(context(paragraph, selected))),
            false,
            None,
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
        let content = Content::from_capture(
            uia(selected, Some(context(paragraph, selected))),
            false,
            None,
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
            let content = Content::from_capture(
                captured(text, None, CaptureMethod::Clipboard),
                pdf,
                None,
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
    }

    #[test]
    fn case_order_follows_the_selection_and_the_sentence_start() {
        let dir = tempfile::tempdir().unwrap();
        let dict = dictionary(dir.path());
        let in_paragraph = |paragraph: &str| {
            Content::from_capture(
                uia("The", Some(context(paragraph, "The"))),
                false,
                None,
                Some(&dict),
            )
        };
        // No sentence: the selected text's own case first.
        let content = Content::from_capture(uia("The", None), false, None, Some(&dict));
        assert_eq!(words(&content), ["The", "the"]);
        // At a sentence start: lowercase first.
        assert_eq!(words(&in_paragraph("The model is large.")), ["the", "The"]);
        // Mid-sentence: its own case first.
        assert_eq!(words(&in_paragraph("We met The model.")), ["The", "the"]);
    }

    #[test]
    fn without_a_dictionary_the_text_is_still_shown() {
        let content = Content::from_capture(uia("transduc\u{2}tion", None), false, None, None);
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
    fn sentence_equal_to_selection_is_dropped() {
        let context = RawContext {
            text: "Hello world.".into(),
            selection: None,
        };
        let content = Content::from_capture(uia("Hello world.", Some(context)), false, None, None);
        assert!(matches!(content, Content::Selection { sentence: None, .. }));
    }

    #[test]
    fn whitespace_only_selection_is_no_selection() {
        assert_eq!(
            Content::from_capture(uia(" \n ", None), false, None, None),
            Content::Unavailable {
                reason: "no_selection"
            }
        );
    }

    #[test]
    fn unavailable_and_error() {
        let unavailable =
            |u| Content::from_capture(Ok(CaptureOutcome::Unavailable(u)), false, None, None);
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
            Content::from_capture(Err(err), false, None, None),
            Content::Error { message } if message.contains("gone")
        ));
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
        assert_eq!(
            serde_json::to_string(&selection).unwrap(),
            r#"{"kind":"selection","text":"qux","sentence":null,"source_app":null,"lookup":{"kind":"not_found"}}"#
        );
    }
}
