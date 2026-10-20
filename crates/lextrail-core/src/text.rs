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
/// several sentences, all of them. Leading and trailing whitespace is trimmed, and so is
/// a leading list marker (`•`, `-`, `*`, `1.`, `(2)`, `a)`, `一、`, `①`, ...) unless the
/// selection itself starts inside it.
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
    let span = &paragraph[bounds[first]..bounds[last.max(first) + 1]];
    let mut text = span.trim();
    let text_start = bounds[first] + (span.len() - span.trim_start().len());
    let rest = strip_list_marker(text);
    let marker_end = text_start + (text.len() - rest.len());
    if !rest.is_empty() && range.start >= marker_end {
        text = rest;
    }
    (!text.is_empty()).then_some(text)
}

/// Bullet characters that may start a list item.
const BULLETS: &[char] = &[
    '\u{2022}', '\u{25e6}', '\u{25aa}', '\u{25ab}', '\u{2023}', '\u{00b7}', '\u{25cf}', '\u{25cb}',
    '\u{25a0}', '\u{25a1}', '\u{27a2}', '\u{25ba}', '-', '*', '+', '\u{2013}', '\u{2014}',
];

/// `text` without leading list markers (up to three, e.g. `• 1. `) and the whitespace
/// after them. A marker only counts when whitespace follows it, except `、` and circled
/// numbers, which are written without a space.
fn strip_list_marker(text: &str) -> &str {
    let mut rest = text;
    for _ in 0..3 {
        match one_marker(rest) {
            Some(after) => rest = after.trim_start(),
            None => break,
        }
    }
    rest
}

/// The text after one leading list marker, or `None` if there is none.
fn one_marker(text: &str) -> Option<&str> {
    let first = text.chars().next()?;
    let followed_by_space = |at: usize| text[at..].starts_with(char::is_whitespace);
    if BULLETS.contains(&first) {
        let at = first.len_utf8();
        return followed_by_space(at).then(|| &text[at..]);
    }
    // ① to ⑳
    if ('\u{2460}'..='\u{2473}').contains(&first) {
        return Some(&text[first.len_utf8()..]);
    }
    // 一、 to 十、
    if "一二三四五六七八九十".contains(first) {
        let after = &text[first.len_utf8()..];
        return after.strip_prefix('、');
    }
    // (1) and （1）
    if first == '(' || first == '\u{ff08}' {
        let inner = &text[first.len_utf8()..];
        let digits = inner.chars().take_while(char::is_ascii_digit).count();
        let after = &inner[digits..];
        let close = after.chars().next()?;
        if (1..=3).contains(&digits) && (close == ')' || close == '\u{ff09}') {
            let at = text.len() - after.len() + close.len_utf8();
            return followed_by_space(at).then(|| &text[at..]);
        }
        return None;
    }
    // 1.  12)  3、  and  a)  B)
    let digits = text.chars().take_while(char::is_ascii_digit).count();
    let (label_len, letter) = if (1..=3).contains(&digits) {
        (digits, false)
    } else if digits == 0 && first.is_ascii_alphabetic() {
        (1, true)
    } else {
        return None;
    };
    let close = text[label_len..].chars().next()?;
    let at = label_len + close.len_utf8();
    match close {
        '、' if !letter => Some(&text[at..]),
        // "A. Lincoln" is a name, not a list item: letters only take ")".
        '.' if !letter => followed_by_space(at).then(|| &text[at..]),
        ')' => followed_by_space(at).then(|| &text[at..]),
        _ => None,
    }
}

/// PDFium (Chrome / Edge PDF viewer) writes this in place of a hyphen at the end of a
/// line. It cannot tell a hyphenation break ("compu-tation") from a compound word's
/// hyphen ("English-to-German"); see [`resolve_line_end_hyphens`].
pub const LINE_END_HYPHEN: char = '\u{2}';

/// Replaces control characters that capture can bring along: [`LINE_END_HYPHEN`] becomes
/// `-` (whitespace right after it is dropped), every other control character (e.g. Word's
/// table cell end `\u{7}`) becomes a space. Ordinary whitespace is left as it is.
pub fn clean_control_chars(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut after_hyphen = false;
    for c in text.chars() {
        if after_hyphen && c.is_whitespace() {
            continue;
        }
        after_hyphen = c == LINE_END_HYPHEN;
        match c {
            LINE_END_HYPHEN => out.push('-'),
            c if c.is_control() && !c.is_whitespace() => out.push(' '),
            c => out.push(c),
        }
    }
    out
}

/// Decides each [`LINE_END_HYPHEN`] between two letters: if the two halves joined form a
/// word (`is_word`, normally the dictionary), the marker is removed ("compu" + "tation" →
/// "computation"); otherwise it stays, and [`clean_control_chars`] later turns it into `-`.
pub fn resolve_line_end_hyphens(text: &str, is_word: impl Fn(&str) -> bool) -> String {
    let mut out = String::with_capacity(text.len());
    let mut rest = text;
    while let Some(at) = rest.find(LINE_END_HYPHEN) {
        let (before, after) = (&rest[..at], &rest[at + LINE_END_HYPHEN.len_utf8()..]);
        let after_trimmed = after.trim_start();
        let left_start = before
            .char_indices()
            .rev()
            .take_while(|(_, c)| c.is_alphabetic())
            .last()
            .map_or(before.len(), |(i, _)| i);
        let right_len: usize = after_trimmed
            .chars()
            .take_while(|c| c.is_alphabetic())
            .map(char::len_utf8)
            .sum();
        let (left, right) = (&before[left_start..], &after_trimmed[..right_len]);
        out.push_str(before);
        if !left.is_empty() && !right.is_empty() && is_word(&format!("{left}{right}")) {
            rest = after_trimmed;
        } else {
            out.push(LINE_END_HYPHEN);
            rest = after;
        }
    }
    out.push_str(rest);
    out
}

/// Joins words that a PDF viewer broke across lines without leaving a mark. Edge's PDF
/// viewer copies "transduc-" / "tion" as `transduc\r\ntion`: the hyphen is gone, so a
/// hyphenation break looks like any other line break. A single line break between two
/// letters is removed when the part before it is not a word on its own and the two parts
/// together are (`is_word`), which leaves ordinary line breaks ("positions\r\nof") alone.
/// Only for text known to come from such a viewer (D044 supplement); text with
/// [`LINE_END_HYPHEN`] markers uses [`resolve_line_end_hyphens`].
pub fn join_unmarked_line_breaks(text: &str, is_word: impl Fn(&str) -> bool) -> String {
    let mut out = String::with_capacity(text.len());
    let mut rest = text;
    while let Some(at) = rest.find(['\r', '\n']) {
        let (before, tail) = rest.split_at(at);
        let break_len = if tail.starts_with("\r\n") { 2 } else { 1 };
        let after = &tail[break_len..];
        let left_start = before
            .char_indices()
            .rev()
            .take_while(|(_, c)| c.is_alphabetic())
            .last()
            .map_or(before.len(), |(i, _)| i);
        let right_len: usize = after
            .chars()
            .take_while(|c| c.is_alphabetic())
            .map(char::len_utf8)
            .sum();
        let (left, right) = (&before[left_start..], &after[..right_len]);
        out.push_str(before);
        let join = !left.is_empty()
            && !right.is_empty()
            && !is_word(left)
            && is_word(&format!("{left}{right}"));
        if !join {
            out.push_str(&tail[..break_len]);
        }
        rest = after;
    }
    out.push_str(rest);
    out
}

/// Whether `selection` is the first word(s) of `sentence`, after opening quotes and
/// brackets: "The" in "The model is ..." or "“The model ...". Used to put the lowercase
/// dictionary entry first for a capitalized word at a sentence start.
pub fn starts_sentence(sentence: &str, selection: &str) -> bool {
    const OPENERS: &[char] = &['"', '\'', '\u{201c}', '\u{2018}', '(', '[', '{'];
    let sentence = sentence.trim_start_matches(|c: char| c.is_whitespace() || OPENERS.contains(&c));
    let selection = selection.trim();
    !selection.is_empty()
        && sentence.starts_with(selection)
        && sentence[selection.len()..]
            .chars()
            .next()
            .is_none_or(|c| !c.is_alphanumeric())
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
        if is_abbreviation(word) || is_list_number(text, chars[start].0 - word.len(), word) {
            return false;
        }
    }
    true
}

/// "1." at the start of a line (after bullets at most) labels a list item.
fn is_list_number(text: &str, word_start: usize, word: &str) -> bool {
    let line_start = text[..word_start].rfind('\n').map_or(0, |i| i + 1);
    (1..=3).contains(&word.len())
        && word.bytes().all(|b| b.is_ascii_digit())
        && text[line_start..word_start]
            .chars()
            .all(|c| c.is_whitespace() || BULLETS.contains(&c))
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

    #[test]
    fn strips_leading_list_markers() {
        let cases = [
            (
                "\u{2022} The Houthis launch item missiles. Next.",
                "The Houthis launch item missiles.",
            ),
            ("- dash item here", "dash item here"),
            ("* star item here", "star item here"),
            ("+ plus item here", "plus item here"),
            ("1. Numbered item here.", "Numbered item here."),
            ("12) Numbered item here.", "Numbered item here."),
            ("(3) Numbered item here.", "Numbered item here."),
            ("a) Lettered item here.", "Lettered item here."),
            ("3、第三项 item here", "第三项 item here"),
            ("二、第二项 item here", "第二项 item here"),
            ("\u{2460}第一项 item here", "第一项 item here"),
            (
                "\u{2022} 1. Nested markers item here.",
                "Nested markers item here.",
            ),
        ];
        for (paragraph, expected) in cases {
            assert_eq!(
                sentence_around(paragraph, "item", None),
                Some(expected),
                "{paragraph}"
            );
        }
    }

    #[test]
    fn keeps_text_that_only_looks_like_a_marker() {
        let cases = [
            "A. Lincoln wrote this item.",
            "-5 degrees item today.",
            "*emphasis* item here.",
            "2024 was an item year.",
            "1.5 item ratio here.",
        ];
        for paragraph in cases {
            assert_eq!(
                sentence_around(paragraph, "item", None),
                Some(paragraph),
                "{paragraph}"
            );
        }
    }

    #[test]
    fn marker_is_kept_when_the_selection_starts_inside_it() {
        assert_eq!(
            sentence_around("1. Item here.", "1.", None),
            Some("1. Item here.")
        );
        // A marker on its own is not stripped to nothing.
        assert_eq!(
            sentence_around("\u{2022}", "\u{2022}", None),
            Some("\u{2022}")
        );
    }

    #[test]
    fn list_markers_after_line_breaks() {
        let p = "Intro paragraph.\n\n\u{2022} Second item here";
        assert_eq!(sentence_around(p, "item", None), Some("Second item here"));
        let q = "Steps:\n1. Open it.\n2. Close the item.";
        assert_eq!(sentence_around(q, "item", None), Some("Close the item."));
    }

    #[test]
    fn numbers_ending_a_sentence_still_split() {
        let p = "The answer is 42. Next item here.";
        assert_eq!(sentence_around(p, "item", None), Some("Next item here."));
    }

    #[test]
    fn hint_whose_text_differs_from_the_selection_is_ignored() {
        // As reported by Edge's PDF viewer: the offset points 3 characters too far, at
        // "ween di", while the selection is "between".
        let p = "This makes it more difficult to learn dependencies between distant \
                 positions [12]. In the Transformer this is reduced.";
        let real = p.find("between").unwrap();
        let shifted = real + 3..real + 3 + "between".len();
        assert_eq!(&p[shifted.clone()], "ween di");
        assert_eq!(
            sentence_around(p, "between", Some(shifted)),
            Some(
                "This makes it more difficult to learn dependencies between distant \
                 positions [12]."
            )
        );
    }

    #[test]
    fn control_characters_are_cleaned() {
        assert_eq!(clean_control_chars("hello world\u{7}"), "hello world ");
        assert_eq!(
            clean_control_chars("English\u{2}to-German"),
            "English-to-German"
        );
        // Whitespace after the marker (a kept line break) is dropped.
        assert_eq!(clean_control_chars("English\u{2}\r\nto"), "English-to");
        // Ordinary whitespace stays for the caller to collapse.
        assert_eq!(clean_control_chars("a\tb\nc"), "a\tb\nc");
        assert_eq!(clean_control_chars("x\u{0}y"), "x y");
    }

    #[test]
    fn line_end_hyphens_are_joined_only_when_the_dictionary_knows_the_word() {
        let words = ["computation", "representation"];
        let is_word = |w: &str| words.contains(&w.to_lowercase().as_str());
        assert_eq!(
            resolve_line_end_hyphens("sequential compu\u{2}tation of", is_word),
            "sequential computation of"
        );
        assert_eq!(
            resolve_line_end_hyphens("Represen\u{2}\r\ntation", is_word),
            "Representation"
        );
        // A compound word keeps its hyphen.
        let kept = resolve_line_end_hyphens("English\u{2}to-German", is_word);
        assert_eq!(kept, "English\u{2}to-German");
        assert_eq!(clean_control_chars(&kept), "English-to-German");
        // Not between letters: kept.
        assert_eq!(
            resolve_line_end_hyphens("2014\u{2}2017", is_word),
            "2014\u{2}2017"
        );
        assert_eq!(resolve_line_end_hyphens("\u{2}", is_word), "\u{2}");
        assert_eq!(
            resolve_line_end_hyphens("no markers", is_word),
            "no markers"
        );
    }

    #[test]
    fn unmarked_line_breaks_are_joined_only_for_word_fragments() {
        // As Edge's PDF viewer copies them (R5): the hyphen is gone.
        let words = [
            "transduction",
            "convolutional",
            "positions",
            "of",
            "sequence",
            "aligned",
            "sequencealigned",
            "multi",
            "multiplicative",
            "english",
            "to",
        ];
        let is_word = |w: &str| words.contains(&w.to_lowercase().as_str());
        assert_eq!(
            join_unmarked_line_breaks(" transduc\r\ntion m", is_word),
            " transduction m"
        );
        assert_eq!(
            join_unmarked_line_breaks("and convolu\ntional layers", is_word),
            "and convolutional layers"
        );
        // Ordinary line breaks and compounds: the part before the break is a word.
        for kept in [
            "positions\r\nof ",
            "sequence\r\naligned",
            "English\r\nto-German",
            // Missed, as the evidence in m1-dict.md shows: "multi" is an entry.
            "multi\r\nplicative",
            // A blank line (paragraph break) and breaks next to non-letters.
            "transduc\r\n\r\ntion",
            "2014\r\n2017",
            "transduc \r\ntion",
        ] {
            assert_eq!(join_unmarked_line_breaks(kept, is_word), kept);
        }
    }

    #[test]
    fn sentence_start() {
        assert!(starts_sentence("The model is large.", "The"));
        assert!(starts_sentence(
            "\u{201c}The model\u{201d} is large.",
            "The"
        ));
        assert!(starts_sentence("March came.", "March came"));
        assert!(!starts_sentence("In March we left.", "March"));
        // A longer word that only begins with the selection.
        assert!(!starts_sentence("Theory says so.", "The"));
        assert!(!starts_sentence("The model.", " "));
    }
}
