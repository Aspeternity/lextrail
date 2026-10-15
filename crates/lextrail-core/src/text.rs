//! Sentence extraction from captured context (ARCHITECTURE.md 4.1).
//!
//! Capture adapters return a whole paragraph or line; this module cuts out the sentence
//! that contains the selection. Pure logic, so the PWA and tests share it.

use std::ops::Range;

/// Words that are followed by a period but do not end a sentence.
const ABBREVIATIONS: &[&str] = &[
    "mr", "mrs", "ms", "dr", "prof", "sr", "jr", "st", "vs", "cf", "fig", "no", "vol", "pp",
    "approx", "dept", "est", "mt", "gen", "col", "lt", "sgt", "rev", "hon",
];

/// Closing quotes and brackets that may follow a sentence terminator.
const CLOSERS: &[char] = &[
    '"', '\'', '\u{201d}', '\u{2019}', ')', ']', '}', '\u{300d}', '\u{300f}', '\u{300b}',
    '\u{ff09}',
];

/// Returns the sentence of `paragraph` that contains `selection`; if the selection spans
/// several sentences, all of them. Leading and trailing whitespace is trimmed.
///
/// `hint` is where the adapter says the selection is (UTF-8 byte offsets). It is used
/// when it is in bounds, on character boundaries and covers text equal to `selection`
/// (ignoring differences in whitespace). Otherwise the selection is searched for:
/// - several matches: the one nearest the hint's start, or the first one without a hint;
/// - no match, or an empty selection: `None` (the caller keeps no context).
pub fn sentence_around<'a>(
    paragraph: &'a str,
    selection: &str,
    hint: Option<Range<usize>>,
) -> Option<&'a str> {
    let range = locate(paragraph, selection, hint)?;
    let bounds = sentence_bounds(paragraph);
    // Sentence i spans bounds[i]..bounds[i + 1].
    let first = bounds.windows(2).position(|w| w[1] > range.start)?;
    let last = bounds
        .windows(2)
        .rposition(|w| w[0] < range.end.max(range.start + 1))?;
    let text = paragraph[bounds[first]..bounds[last.max(first) + 1]].trim();
    (!text.is_empty()).then_some(text)
}

/// Finds the byte range of `selection` in `paragraph` (see [`sentence_around`]).
fn locate(paragraph: &str, selection: &str, hint: Option<Range<usize>>) -> Option<Range<usize>> {
    let selection = selection.trim();
    if selection.is_empty() {
        return None;
    }
    if let Some(h) = &hint
        && h.start < h.end
        && h.end <= paragraph.len()
        && paragraph.is_char_boundary(h.start)
        && paragraph.is_char_boundary(h.end)
        && same_words(&paragraph[h.clone()], selection)
    {
        return Some(h.clone());
    }
    let near = hint.map_or(0, |h| h.start);
    paragraph
        .match_indices(selection)
        .map(|(start, s)| start..start + s.len())
        .min_by_key(|r| r.start.abs_diff(near))
}

fn same_words(a: &str, b: &str) -> bool {
    a.split_whitespace().eq(b.split_whitespace())
}

/// Byte offsets where sentences start, plus `text.len()` at the end. Always begins with 0.
fn sentence_bounds(text: &str) -> Vec<usize> {
    let mut bounds = vec![0];
    let chars: Vec<(usize, char)> = text.char_indices().collect();
    let mut i = 0;
    while i < chars.len() {
        let (_, c) = chars[i];
        if c == '\n' {
            // A blank line separates paragraphs; a single line break (hard wrap in PDFs,
            // terminals) does not end a sentence.
            let mut j = i + 1;
            while j < chars.len() && chars[j].1 != '\n' && chars[j].1.is_whitespace() {
                j += 1;
            }
            if j < chars.len() && chars[j].1 == '\n' {
                push_bound(&mut bounds, end_of(&chars, j, text));
                i = j + 1;
                continue;
            }
        } else if is_cjk_terminator(c) || is_latin_terminator(c) {
            let mut j = i;
            while j < chars.len()
                && (is_cjk_terminator(chars[j].1) || is_latin_terminator(chars[j].1))
            {
                j += 1;
            }
            while j < chars.len() && CLOSERS.contains(&chars[j].1) {
                j += 1;
            }
            let ends = if is_cjk_terminator(c) {
                true
            } else {
                latin_sentence_ends(text, &chars, i, j)
            };
            if ends {
                push_bound(&mut bounds, end_of(&chars, j, text));
            }
            i = j;
            continue;
        }
        i += 1;
    }
    if *bounds.last().unwrap_or(&0) != text.len() {
        bounds.push(text.len());
    }
    bounds
}

fn push_bound(bounds: &mut Vec<usize>, at: usize) {
    if bounds.last() != Some(&at) {
        bounds.push(at);
    }
}

/// Byte offset of `chars[j]`, or the end of the text.
fn end_of(chars: &[(usize, char)], j: usize, text: &str) -> usize {
    chars.get(j).map_or(text.len(), |&(at, _)| at)
}

fn is_cjk_terminator(c: char) -> bool {
    matches!(c, '\u{3002}' | '\u{ff01}' | '\u{ff1f}')
}

fn is_latin_terminator(c: char) -> bool {
    matches!(c, '.' | '!' | '?' | '\u{2026}')
}

/// Whether Latin terminators at `chars[start..end]` (closers included) end a sentence.
fn latin_sentence_ends(text: &str, chars: &[(usize, char)], start: usize, end: usize) -> bool {
    // Must be followed by whitespace or the end of the text ("3.14", "example.com").
    match chars.get(end) {
        None => return true,
        Some(&(_, c)) if !c.is_whitespace() => return false,
        _ => {}
    }
    // "e.g. the", "Wow!" she said: a lowercase continuation is the same sentence.
    if let Some(&(_, next)) = chars[end..].iter().find(|(_, c)| !c.is_whitespace())
        && next.is_lowercase()
    {
        return false;
    }
    if chars[start].1 == '.' && end - start == 1 + count_closers(chars, start + 1, end) {
        let word = word_before(text, chars[start].0);
        if is_abbreviation(word) {
            return false;
        }
    }
    true
}

fn count_closers(chars: &[(usize, char)], from: usize, to: usize) -> usize {
    chars[from..to]
        .iter()
        .filter(|(_, c)| CLOSERS.contains(c))
        .count()
}

/// The letters, digits and dots directly before byte offset `at`.
fn word_before(text: &str, at: usize) -> &str {
    let head = &text[..at];
    let start = head
        .char_indices()
        .rev()
        .find(|&(_, c)| !(c.is_alphanumeric() || c == '.'))
        .map_or(0, |(i, c)| i + c.len_utf8());
    &head[start..]
}

fn is_abbreviation(word: &str) -> bool {
    let mut chars = word.chars();
    // Initials: "J. K. Rowling".
    if let (Some(c), None) = (chars.next(), chars.next())
        && c.is_uppercase()
    {
        return true;
    }
    // Dotted abbreviations: "e.g", "i.e", "U.S", "a.m".
    if word.contains('.') {
        return true;
    }
    ABBREVIATIONS.contains(&word.to_lowercase().as_str())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn around(paragraph: &str, selection: &str) -> Option<String> {
        sentence_around(paragraph, selection, None).map(str::to_owned)
    }

    #[test]
    fn picks_the_sentence_with_the_selection() {
        let p = "First one. We must address the root cause! Is it done? Yes.";
        assert_eq!(
            around(p, "address").as_deref(),
            Some("We must address the root cause!")
        );
        assert_eq!(around(p, "done").as_deref(), Some("Is it done?"));
        assert_eq!(around(p, "First").as_deref(), Some("First one."));
        assert_eq!(around(p, "Yes").as_deref(), Some("Yes."));
    }

    #[test]
    fn selection_across_sentences_returns_all_of_them() {
        let p = "One two. Three four. Five six.";
        assert_eq!(
            around(p, "two. Three").as_deref(),
            Some("One two. Three four.")
        );
    }

    #[test]
    fn handles_chinese_terminators() {
        let p = "他说：address 是地址。但这里是处理的意思！对吗？";
        assert_eq!(around(p, "处理").as_deref(), Some("但这里是处理的意思！"));
        assert_eq!(
            around(p, "address").as_deref(),
            Some("他说：address 是地址。")
        );
    }

    #[test]
    fn does_not_split_decimals_urls_or_abbreviations() {
        let p = "Pi is 3.14 and see example.com for more. Mr. Smith met Dr. Jones vs. the U.S. team. Done.";
        assert_eq!(
            around(p, "example").as_deref(),
            Some("Pi is 3.14 and see example.com for more.")
        );
        assert_eq!(
            around(p, "Jones").as_deref(),
            Some("Mr. Smith met Dr. Jones vs. the U.S. team.")
        );
    }

    #[test]
    fn does_not_split_initials_or_lowercase_continuations() {
        let p = "Written by J. K. Rowling, e.g. the first book. Next.";
        assert_eq!(
            around(p, "Rowling").as_deref(),
            Some("Written by J. K. Rowling, e.g. the first book.")
        );
        let q = "\"Wow!\" she said. Then left.";
        assert_eq!(around(q, "said").as_deref(), Some("\"Wow!\" she said."));
    }

    #[test]
    fn handles_ellipses_and_closing_quotes() {
        let p = "He waited... and waited… Then he left. \"Is it over?\" She nodded.";
        assert_eq!(
            around(p, "and").as_deref(),
            Some("He waited... and waited…")
        );
        assert_eq!(around(p, "over").as_deref(), Some("\"Is it over?\""));
        assert_eq!(around(p, "nodded").as_deref(), Some("She nodded."));
    }

    #[test]
    fn single_line_breaks_do_not_end_sentences_but_blank_lines_do() {
        let p = "A hard-wrapped line\nthat goes on here\n\nA heading without period\nNext para.";
        assert_eq!(
            around(p, "goes").as_deref(),
            Some("A hard-wrapped line\nthat goes on here")
        );
        assert_eq!(
            around(p, "heading").as_deref(),
            Some("A heading without period\nNext para.")
        );
    }

    #[test]
    fn whole_paragraph_without_terminators() {
        assert_eq!(
            around("  just a phrase  ", "phrase").as_deref(),
            Some("just a phrase")
        );
    }

    #[test]
    fn valid_hint_picks_that_occurrence() {
        let p = "Set it here. Then set it there.";
        let second = p.rfind("set").unwrap();
        assert_eq!(
            sentence_around(p, "set", Some(second..second + 3)),
            Some("Then set it there.")
        );
        // Whitespace differences between the hint's text and the selection are fine.
        let q = "Look at\nthis. Other.";
        assert_eq!(
            sentence_around(q, "at this", Some(5..12)),
            Some("Look at\nthis.")
        );
    }

    #[test]
    fn invalid_hints_fall_back_to_search_without_panicking() {
        let p = "Zero. 词语在这里。 One word here. Another word there.";
        let near_end = p.len() - 3;
        let cases = [
            Some(0..p.len() + 10), // out of bounds
            Some(7..8),            // inside a multi-byte character
            #[allow(clippy::reversed_empty_ranges)]
            Some(5..2), // reversed
            Some(0..4),            // text does not match
            Some(near_end..near_end), // empty
            Some(usize::MAX - 1..usize::MAX), // absurd
        ];
        for hint in cases {
            let got = sentence_around(p, "word", hint.clone());
            assert!(got.is_some(), "{hint:?}");
        }
        // Mismatched hint near the end: the nearest occurrence is used.
        assert_eq!(
            sentence_around(p, "word", Some(near_end..near_end + 1)),
            Some("Another word there.")
        );
        // No hint: the first occurrence.
        assert_eq!(sentence_around(p, "word", None), Some("One word here."));
    }

    #[test]
    fn missing_or_empty_selection_gives_none() {
        assert_eq!(sentence_around("Some text here.", "absent", None), None);
        assert_eq!(sentence_around("Some text here.", "  ", None), None);
        assert_eq!(sentence_around("", "x", None), None);
    }
}
