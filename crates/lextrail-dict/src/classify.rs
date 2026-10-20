//! Input classification: word, phrase or sentence (`docs/specs/m1-dict.md` section 11,
//! D045). Words and phrases go to the dictionary and the AI's reading in context; sentences
//! go to AI translation.

use lextrail_core::lookup::LookupKind;

use crate::normalize_query;

/// A phrase has at most this many words, unless the dictionary has it as a whole.
pub const MAX_PHRASE_WORDS: usize = 5;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Classification {
    English(LookupKind),
    /// No Latin letter at all (Chinese, digits, symbols): not looked up, not recorded; the
    /// popup says only English is supported for now.
    NotEnglish,
}

/// Classifies selected text. `in_dictionary` says whether a multi-word text is a
/// dictionary entry as a whole (normally [`crate::Dictionary::lookup`]; it is only asked
/// about texts with several words).
///
/// - No ASCII letter → [`Classification::NotEnglish`].
/// - One word (hyphenated compounds and `e.g.` included) → word.
/// - Several words: in the dictionary as a whole, or at most [`MAX_PHRASE_WORDS`] words and
///   not ending with a sentence terminator (`.` `!` `?` `…` and their Chinese forms, before
///   closing quotes and brackets) → phrase; otherwise → sentence.
pub fn classify(text: &str, in_dictionary: impl FnOnce(&str) -> bool) -> Classification {
    if !text.bytes().any(|b| b.is_ascii_alphabetic()) {
        return Classification::NotEnglish;
    }
    let query = normalize_query(text);
    let words = query.split(' ').filter(|w| !w.is_empty()).count();
    let kind = if words <= 1 {
        LookupKind::Word
    } else if in_dictionary(&query) || (words <= MAX_PHRASE_WORDS && !ends_like_a_sentence(text)) {
        LookupKind::Phrase
    } else {
        LookupKind::Sentence
    };
    Classification::English(kind)
}

/// Ends with `.` `!` `?` `…` (or `。！？`), ignoring trailing spaces, closing quotes and
/// brackets.
fn ends_like_a_sentence(text: &str) -> bool {
    let closers = |c: char| {
        c.is_whitespace() || matches!(c, '"' | '\'' | ')' | ']' | '”' | '’' | '）' | '」' | '』')
    };
    text.trim_end_matches(closers)
        .ends_with(['.', '!', '?', '…', '。', '！', '？'])
}

#[cfg(test)]
mod tests {
    use super::*;
    use Classification::{English, NotEnglish};
    use LookupKind::{Phrase, Sentence, Word};

    fn kind(text: &str) -> Classification {
        // Only "give up" and "in terms of" are dictionary phrases here.
        classify(text, |q| q == "give up" || q == "in terms of")
    }

    #[test]
    fn single_words() {
        assert_eq!(kind("launch"), English(Word));
        assert_eq!(kind("  “Transformer”, "), English(Word));
        assert_eq!(kind("self-attention"), English(Word));
        assert_eq!(kind("English-to-German"), English(Word));
        assert_eq!(kind("model's"), English(Word));
        assert_eq!(kind("e.g."), English(Word));
        // A capitalized word with a full stop is still one word.
        assert_eq!(kind("Hello."), English(Word));
    }

    #[test]
    fn dictionary_phrases_are_phrases_whatever_their_ending() {
        assert_eq!(kind("give up"), English(Phrase));
        assert_eq!(kind("in terms of."), English(Phrase));
        assert_eq!(kind("give\n  up"), English(Phrase));
    }

    #[test]
    fn short_word_groups_are_phrases() {
        assert_eq!(kind("multi-head attention"), English(Phrase));
        assert_eq!(kind("the multi-head attention layer"), English(Phrase));
        // Five words: still a phrase.
        assert_eq!(kind("Attention Is All You Need"), English(Phrase));
    }

    #[test]
    fn longer_or_terminated_texts_are_sentences() {
        // Six words.
        assert_eq!(
            kind("the encoder maps an input sequence"),
            English(Sentence)
        );
        // Short but ends like a sentence.
        assert_eq!(kind("We use dropout."), English(Sentence));
        assert_eq!(kind("Is it true?"), English(Sentence));
        assert_eq!(kind("It works!\" "), English(Sentence));
        assert_eq!(kind("(see below.)"), English(Sentence));
        assert_eq!(kind("Wait…"), English(Word));
        assert_eq!(kind("wait for it…"), English(Sentence));
        assert_eq!(kind("Two sentences. Here."), English(Sentence));
    }

    #[test]
    fn the_dictionary_is_only_asked_about_several_words() {
        let mut asked = Vec::new();
        classify("launch", |q| {
            asked.push(q.to_owned());
            false
        });
        assert!(asked.is_empty());
        classify("  give \n up ", |q| {
            asked.push(q.to_owned());
            true
        });
        assert_eq!(asked, ["give up"]);
    }

    #[test]
    fn text_without_latin_letters_is_not_english() {
        assert_eq!(kind("注意力机制"), NotEnglish);
        assert_eq!(kind("2014–2017"), NotEnglish);
        assert_eq!(kind("  —  "), NotEnglish);
        assert_eq!(kind(""), NotEnglish);
        // Mixed text with English letters is classified as usual.
        assert_eq!(kind("注意力 attention"), English(Phrase));
    }
}
