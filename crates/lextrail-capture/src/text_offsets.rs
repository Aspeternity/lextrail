//! Converting native text offsets to the UTF-8 byte offsets of [`crate::RawContext`].

use std::ops::Range;

/// Converts a range in UTF-16 code units of `text` into UTF-8 byte offsets.
///
/// The result always falls on character boundaries: a start inside a surrogate pair moves
/// back to the start of the character, an end inside one moves forward to its end.
/// Returns `None` if the range is reversed or reaches past the end of `text`.
pub fn utf16_range_to_utf8(text: &str, range: Range<usize>) -> Option<Range<usize>> {
    if range.start > range.end {
        return None;
    }
    let mut start = None;
    let mut units = 0;
    for (byte, c) in text.char_indices() {
        let next = units + c.len_utf16();
        if start.is_none() && range.start < next {
            start = Some(byte);
        }
        if range.end <= units {
            return Some(start.unwrap_or(byte)..byte);
        }
        units = next;
    }
    (range.end <= units).then(|| start.unwrap_or(text.len())..text.len())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ascii_offsets_are_unchanged() {
        assert_eq!(utf16_range_to_utf8("hello world", 6..11), Some(6..11));
        assert_eq!(utf16_range_to_utf8("hello", 0..0), Some(0..0));
        assert_eq!(utf16_range_to_utf8("hello", 5..5), Some(5..5));
    }

    #[test]
    fn multi_byte_characters() {
        // "词" is 1 UTF-16 unit and 3 UTF-8 bytes.
        let text = "词语 address";
        let got = utf16_range_to_utf8(text, 3..10).unwrap();
        assert_eq!(&text[got], "address");
    }

    #[test]
    fn surrogate_pairs_round_outwards() {
        // "😀" is 2 UTF-16 units and 4 UTF-8 bytes.
        let text = "a😀b";
        assert_eq!(utf16_range_to_utf8(text, 1..3), Some(1..5));
        assert_eq!(utf16_range_to_utf8(text, 2..3), Some(1..5));
        assert_eq!(utf16_range_to_utf8(text, 1..2), Some(1..5));
        assert_eq!(utf16_range_to_utf8(text, 3..4), Some(5..6));
    }

    #[test]
    fn rejects_out_of_range_and_reversed() {
        assert_eq!(utf16_range_to_utf8("abc", 1..4), None);
        assert_eq!(utf16_range_to_utf8("abc", 4..4), None);
        #[allow(clippy::reversed_empty_ranges)]
        let reversed = 2..1;
        assert_eq!(utf16_range_to_utf8("abc", reversed), None);
        assert_eq!(utf16_range_to_utf8("", 0..0), Some(0..0));
    }
}
