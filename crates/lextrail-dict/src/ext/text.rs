//! Text rules of the extended dictionary: sense fingerprints (D052) and the folding used
//! to match lookup keys with headwords and forms (`docs/specs/m1.5-oxford.md` 5.1, 6.1).

use sha2::{Digest, Sha256};
use unicode_normalization::UnicodeNormalization;
use unicode_normalization::char::is_combining_mark;

/// NFC, surrounding whitespace removed, inner runs of whitespace collapsed to one space.
fn tidy(text: &str) -> String {
    let nfc: String = text.nfc().collect();
    nfc.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// The first 16 hex digits of the SHA-256 of the tidied text.
pub fn text_hash(text: &str) -> String {
    Sha256::digest(tidy(text).as_bytes())
        .iter()
        .take(8)
        .map(|b| format!("{b:02x}"))
        .collect()
}

/// A sense's fingerprint, used to find it again when the source is replaced by another
/// export: the hash of its English definition, or, for a sense without one, `alt:` and the
/// hash of its English text (everything in the sense but Chinese and nested idioms).
pub fn sense_fingerprint(definition: Option<&str>, english_text: &str) -> String {
    match definition.map(tidy).filter(|d| !d.is_empty()) {
        Some(d) => text_hash(&d),
        None => format!("alt:{}", text_hash(english_text)),
    }
}

/// Folds text for matching a lookup key with a headword or form: accents removed,
/// lowercase, without spaces, hyphens, apostrophes and quotation marks, full stops,
/// slashes, parentheses, syllable dots, stress marks and trademark signs. `Self-Help`, `self help` and `selfhelp` all fold to
/// `selfhelp`; `café` to `cafe`.
pub fn fold_for_match(text: &str) -> String {
    // Before NFKD, which spells `™` as `TM`.
    text.chars()
        .filter(|c| !matches!(c, '™' | '®'))
        .collect::<String>()
        .nfkd()
        .filter(|&c| !is_combining_mark(c))
        .filter(|&c| {
            !c.is_whitespace()
                && !matches!(
                    c,
                    '-' | '\u{2010}'
                        | '\u{2011}'
                        | '\''
                        | '\u{2018}'
                        | '\u{2019}'
                        | '"'
                        | '\u{201C}'
                        | '\u{201D}'
                        | '.'
                        | '/'
                        | '('
                        | ')'
                        | '·'
                        | 'ˈ'
                        | 'ˌ'
                )
        })
        .flat_map(char::to_lowercase)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hash_ignores_whitespace_differences_and_normalization() {
        let h = text_hash("to make a blorft");
        assert_eq!(h.len(), 16);
        assert!(h.chars().all(|c| c.is_ascii_hexdigit()));
        assert_eq!(text_hash("  to make\na   blorft "), h);
        assert_ne!(text_hash("to make a blorft."), h);
        // "é" as one code point and as "e" + combining accent
        assert_eq!(text_hash("caf\u{e9}"), text_hash("cafe\u{301}"));
    }

    #[test]
    fn fingerprint_uses_definition_or_marked_alternative() {
        let def = sense_fingerprint(Some("to make a blorft"), "ignored");
        assert_eq!(def, text_hash("to make a blorft"));
        let alt = sense_fingerprint(None, "see snib");
        assert_eq!(alt, format!("alt:{}", text_hash("see snib")));
        assert_eq!(sense_fingerprint(Some("  "), "see snib"), alt);
    }

    #[test]
    fn folding() {
        for (a, b) in [
            ("Self-Help", "selfhelp"),
            ("self help", "selfhelp"),
            ("café", "cafe"),
            ("o’clock", "oclock"),
            ("Mr.", "mr"),
            ("and/or", "andor"),
            ("blorf(e)", "blorfe"),
            ("blor·fing", "blorfing"),
            ("the royal ‘snib’", "theroyalsnib"),
            ("Snib-Blorf™", "snibblorf"),
        ] {
            assert_eq!(fold_for_match(a), b, "{a}");
        }
    }
}
