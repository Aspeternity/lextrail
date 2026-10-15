//! What the popup page displays, sent to it as the `popup:content` event.

use lextrail_capture::{CaptureError, CaptureOutcome, Unavailable};
use lextrail_core::lookup::normalize_text;
use lextrail_core::text::sentence_around;
use serde::Serialize;

pub const EVENT: &str = "popup:content";

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Content {
    /// Nothing; sent when the popup hides so a stale lookup never flashes up again.
    Empty,
    /// Capture is taking a while.
    Loading,
    Selection {
        text: String,
        sentence: Option<String>,
        source_app: Option<String>,
    },
    Unavailable {
        reason: &'static str,
    },
    Error {
        message: String,
    },
}

impl Content {
    pub fn from_capture(
        result: Result<CaptureOutcome, CaptureError>,
        source_app: Option<String>,
    ) -> Self {
        match result {
            Ok(CaptureOutcome::Captured(selection)) => {
                let text = normalize_text(&selection.text);
                if text.is_empty() {
                    return Self::Unavailable {
                        reason: reason(Unavailable::NoSelection),
                    };
                }
                let sentence = selection
                    .context
                    .as_ref()
                    .and_then(|c| sentence_around(&c.text, &selection.text, c.selection.clone()))
                    .map(normalize_text)
                    .filter(|s| !s.is_empty() && *s != text);
                Self::Selection {
                    text,
                    sentence,
                    source_app,
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
    }
}

#[cfg(test)]
mod tests {
    use lextrail_capture::{CaptureMethod, RawContext, Selection};

    use super::*;

    fn captured(text: &str, context: Option<RawContext>) -> Result<CaptureOutcome, CaptureError> {
        Ok(CaptureOutcome::Captured(Selection {
            text: text.into(),
            context,
            url: None,
            method: CaptureMethod::Uia,
        }))
    }

    #[test]
    fn selection_with_sentence() {
        let context = RawContext {
            text: "First. We must  address it. Last.".into(),
            selection: Some(16..23),
        };
        let content = Content::from_capture(
            captured(" address ", Some(context)),
            Some("chrome.exe".into()),
        );
        assert_eq!(
            content,
            Content::Selection {
                text: "address".into(),
                sentence: Some("We must address it.".into()),
                source_app: Some("chrome.exe".into()),
            }
        );
    }

    #[test]
    fn sentence_equal_to_selection_is_dropped() {
        let context = RawContext {
            text: "Hello world.".into(),
            selection: None,
        };
        let content = Content::from_capture(captured("Hello world.", Some(context)), None);
        assert!(matches!(content, Content::Selection { sentence: None, .. }));
    }

    #[test]
    fn whitespace_only_selection_is_no_selection() {
        assert_eq!(
            Content::from_capture(captured(" \n ", None), None),
            Content::Unavailable {
                reason: "no_selection"
            }
        );
    }

    #[test]
    fn unavailable_and_error() {
        assert_eq!(
            Content::from_capture(Ok(CaptureOutcome::Unavailable(Unavailable::Elevated)), None),
            Content::Unavailable { reason: "elevated" }
        );
        let err = CaptureError::WorkerUnavailable("gone".into());
        assert!(matches!(
            Content::from_capture(Err(err), None),
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
    }
}
