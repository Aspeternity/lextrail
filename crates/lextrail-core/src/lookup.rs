//! Lookup history model (ARCHITECTURE.md 6.3, D039).

use std::fmt;
use std::str::FromStr;

/// Longest `query` / `context` kept, in characters.
pub const MAX_TEXT_CHARS: usize = 2000;

/// The same query and context within this window is recorded only once.
pub const DEDUP_WINDOW_MS: u64 = 30_000;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum LookupKind {
    Word,
    Phrase,
    Sentence,
}

/// How the text was captured. Stored values match the `CHECK` constraint on `lookups.method`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum CaptureMethod {
    Uia,
    Clipboard,
    ClipboardDoubleCopy,
    Ocr,
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("unknown {what} {value:?}")]
pub struct UnknownValue {
    what: &'static str,
    value: String,
}

macro_rules! text_enum {
    ($ty:ident, $what:literal, { $($variant:ident => $text:literal),+ $(,)? }) => {
        impl $ty {
            pub const ALL: &[Self] = &[$(Self::$variant),+];

            pub fn as_str(self) -> &'static str {
                match self {
                    $(Self::$variant => $text),+
                }
            }
        }

        impl fmt::Display for $ty {
            fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                f.write_str(self.as_str())
            }
        }

        impl FromStr for $ty {
            type Err = UnknownValue;

            fn from_str(s: &str) -> Result<Self, Self::Err> {
                match s {
                    $($text => Ok(Self::$variant),)+
                    _ => Err(UnknownValue { what: $what, value: s.to_owned() }),
                }
            }
        }
    };
}

text_enum!(LookupKind, "lookup kind", {
    Word => "word",
    Phrase => "phrase",
    Sentence => "sentence",
});

text_enum!(CaptureMethod, "capture method", {
    Uia => "uia",
    Clipboard => "clipboard",
    ClipboardDoubleCopy => "clipboard_double_copy",
    Ocr => "ocr",
});

/// Collapses runs of whitespace to one space, trims, and keeps at most
/// [`MAX_TEXT_CHARS`] characters. Control characters are cleaned up first with
/// [`crate::text::clean_control_chars`] (line-end hyphen marker → `-`, others → space).
pub fn normalize_text(text: &str) -> String {
    let cleaned = crate::text::clean_control_chars(text);
    let mut out = String::with_capacity(cleaned.len().min(MAX_TEXT_CHARS * 4));
    let mut chars = 0;
    for word in cleaned.split_whitespace() {
        let sep = usize::from(chars > 0);
        if chars + sep >= MAX_TEXT_CHARS {
            break;
        }
        if sep == 1 {
            out.push(' ');
            chars += 1;
        }
        for c in word.chars() {
            if chars == MAX_TEXT_CHARS {
                break;
            }
            out.push(c);
            chars += 1;
        }
    }
    out
}

/// Whether a lookup at `now_ms` repeats one with the same query and context recorded at
/// `previous_ms`. Uses the distance in either direction, so a clock moved back does not
/// make every later lookup look like a duplicate.
pub fn is_repeat(previous_ms: u64, now_ms: u64) -> bool {
    previous_ms.abs_diff(now_ms) < DEDUP_WINDOW_MS
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn enums_round_trip_through_text() {
        for k in LookupKind::ALL {
            assert_eq!(k.as_str().parse::<LookupKind>().unwrap(), *k);
        }
        for m in CaptureMethod::ALL {
            assert_eq!(m.to_string().parse::<CaptureMethod>().unwrap(), *m);
        }
        assert_eq!(
            CaptureMethod::ClipboardDoubleCopy.as_str(),
            "clipboard_double_copy"
        );
        assert!("Word".parse::<LookupKind>().is_err());
        assert!("".parse::<CaptureMethod>().is_err());
    }

    #[test]
    fn normalize_collapses_whitespace() {
        assert_eq!(
            normalize_text("  take\n off \t\u{3000}now "),
            "take off now"
        );
        assert_eq!(normalize_text(" \n\t"), "");
    }

    #[test]
    fn normalize_removes_control_characters() {
        // Word table cell end marker.
        assert_eq!(normalize_text("hello world\u{7}"), "hello world");
        // PDFium line-end hyphen marker.
        assert_eq!(
            normalize_text("the English\u{2}to-German task"),
            "the English-to-German task"
        );
        assert_eq!(normalize_text("a\u{0}b\u{1b}c"), "a b c");
    }

    #[test]
    fn normalize_truncates_by_characters() {
        let long = "字".repeat(MAX_TEXT_CHARS + 10);
        let got = normalize_text(&long);
        assert_eq!(got.chars().count(), MAX_TEXT_CHARS);

        let words = "ab ".repeat(MAX_TEXT_CHARS);
        let got = normalize_text(&words);
        assert_eq!(got.chars().count(), MAX_TEXT_CHARS);
        assert!(!got.ends_with(' '));
    }

    #[test]
    fn repeat_window() {
        assert!(is_repeat(1000, 1000));
        assert!(is_repeat(1000, 1000 + DEDUP_WINDOW_MS - 1));
        assert!(!is_repeat(1000, 1000 + DEDUP_WINDOW_MS));
        // Clock moved back far: not a repeat.
        assert!(!is_repeat(100_000, 1000));
    }
}
