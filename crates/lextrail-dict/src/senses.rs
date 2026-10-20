//! Splitting ECDICT's Chinese `translation` field into separate senses
//! (`docs/specs/m1-dict.md` section 4).

/// One sense of an entry.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Sense {
    /// Part of speech, e.g. `n.`, `vt.`; several are joined as `vt. & vi.` whatever the
    /// source wrote (`vt.,vi.`, `vt.& vi.`, `n./v.`). `None` when the line has none.
    pub pos: Option<String>,
    /// Domain label without brackets, e.g. `医`, `网络`.
    pub domain: Option<String>,
    pub text: String,
}

/// Part-of-speech abbreviations used in ECDICT. Only these are taken as a POS prefix, so
/// that a line starting with ordinary text is never cut.
const POS: &[&str] = &[
    "a", "ad", "adj", "adv", "abbr", "art", "aux", "comb", "conj", "det", "int", "interj", "n",
    "num", "phr", "pl", "pref", "prep", "pron", "suf", "suff", "v", "vi", "vt",
];

/// Characters that join several POS, as in `vt.& vi.` or `n./v.`.
const POS_JOINERS: [char; 3] = ['&', '/', ','];

/// Splits a raw `translation` value. Lines are separated by a literal `\n` (as stored in
/// the CSV) or a real newline.
pub fn split_translation(raw: &str) -> Vec<Sense> {
    let mut senses = Vec::new();
    for line in raw.split("\\n").flat_map(str::lines) {
        let (domain, rest) = take_domain(line.trim());
        let (pos, body) = take_pos(rest);
        for text in split_line(body) {
            senses.push(Sense {
                pos: pos.clone(),
                domain: domain.clone(),
                text,
            });
        }
    }
    senses
}

/// `[医] 原, 剂` -> (`Some("医")`, `原, 剂`).
fn take_domain(line: &str) -> (Option<String>, &str) {
    if let Some(inner) = line.strip_prefix('[')
        && let Some(end) = inner.find(']')
    {
        let label = inner[..end].trim();
        if !label.is_empty() {
            return (Some(label.to_owned()), inner[end + 1..].trim_start());
        }
    }
    (None, line)
}

/// `vt.,vi. 推` -> (`Some("vt. & vi.")`, `推`). Each POS must come from [`POS`], end in a
/// dot and be followed by a space, a joiner or the end of the line (so `a.m.` is text).
fn take_pos(line: &str) -> (Option<String>, &str) {
    let mut start = 0;
    let mut pos_end = None;
    let mut tokens = Vec::new();
    loop {
        let rest = &line[start..];
        let letters = rest.bytes().take_while(u8::is_ascii_lowercase).count();
        if letters == 0 || rest.as_bytes().get(letters) != Some(&b'.') {
            break;
        }
        if !POS.contains(&&rest[..letters]) {
            break;
        }
        let end = start + letters + 1;
        let next = line[end..].chars().next();
        if next.is_some_and(|c| !c.is_whitespace() && !POS_JOINERS.contains(&c)) {
            break;
        }
        pos_end = Some(end);
        tokens.push(&line[start..end]);
        let Some(joined) = line[end..].trim_start().strip_prefix(POS_JOINERS) else {
            break;
        };
        start = line.len() - joined.trim_start().len();
    }
    match pos_end {
        Some(end) => (Some(tokens.join(" & ")), line[end..].trim_start()),
        None => (None, line),
    }
}

/// Splits one line's body: on semicolons if it has any outside brackets, otherwise on commas.
fn split_line(body: &str) -> Vec<String> {
    let is_semicolon = |c: char| c == ';' || c == '；';
    let by_semicolon = separators(body).any(|(_, c)| is_semicolon(c));
    let mut parts = Vec::new();
    let mut start = 0;
    for (i, c) in separators(body) {
        if is_semicolon(c) == by_semicolon {
            parts.push(&body[start..i]);
            start = i + c.len_utf8();
        }
    }
    parts.push(&body[start..]);
    parts
        .into_iter()
        .map(str::trim)
        .filter(|p| !p.is_empty())
        .map(str::to_owned)
        .collect()
}

/// Separator characters (`,` `，` `;` `；`) outside brackets, with their byte offsets.
/// A `,` between two digits (`3,000`) is not a separator.
fn separators(body: &str) -> impl Iterator<Item = (usize, char)> + '_ {
    let mut depth = 0usize;
    let mut prev: Option<char> = None;
    let mut chars = body.char_indices().peekable();
    std::iter::from_fn(move || {
        while let Some((i, c)) = chars.next() {
            let before = prev.replace(c);
            match c {
                '(' | '（' | '[' | '【' => depth += 1,
                ')' | '）' | ']' | '】' => depth = depth.saturating_sub(1),
                ',' if depth == 0 => {
                    let between_digits = before.is_some_and(|b| b.is_ascii_digit())
                        && chars.peek().is_some_and(|&(_, n)| n.is_ascii_digit());
                    if !between_digits {
                        return Some((i, c));
                    }
                }
                '，' | ';' | '；' if depth == 0 => return Some((i, c)),
                _ => {}
            }
        }
        None
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sense(pos: Option<&str>, domain: Option<&str>, text: &str) -> Sense {
        Sense {
            pos: pos.map(str::to_owned),
            domain: domain.map(str::to_owned),
            text: text.to_owned(),
        }
    }

    fn texts(raw: &str) -> Vec<String> {
        split_translation(raw).into_iter().map(|s| s.text).collect()
    }

    #[test]
    fn semicolons_split_and_commas_stay_within_a_sense() {
        assert_eq!(
            texts("v. 为王，为君；当政；统治"),
            ["为王，为君", "当政", "统治"]
        );
    }

    #[test]
    fn lines_without_semicolons_split_on_commas() {
        assert_eq!(
            texts("a. 真的, 真实的, 实际的, 实在的, 不动(产)的, 实数的"),
            ["真的", "真实的", "实际的", "实在的", "不动(产)的", "实数的"]
        );
    }

    #[test]
    fn each_line_keeps_its_pos() {
        assert_eq!(
            split_translation(r"n. 实数, 现实\nadv. 真正地"),
            [
                sense(Some("n."), None, "实数"),
                sense(Some("n."), None, "现实"),
                sense(Some("adv."), None, "真正地"),
            ]
        );
    }

    #[test]
    fn domain_labels_are_taken_off() {
        assert_eq!(
            split_translation(r"n. 玄武岩\n[化] 玄武岩"),
            [
                sense(Some("n."), None, "玄武岩"),
                sense(None, Some("化"), "玄武岩"),
            ]
        );
        assert_eq!(
            split_translation("[网络] 胡德；兜帽"),
            [
                sense(None, Some("网络"), "胡德"),
                sense(None, Some("网络"), "兜帽"),
            ]
        );
    }

    #[test]
    fn domain_then_pos() {
        assert_eq!(
            split_translation("[计] n. 地址, 寻址"),
            [
                sense(Some("n."), Some("计"), "地址"),
                sense(Some("n."), Some("计"), "寻址"),
            ]
        );
    }

    #[test]
    fn joined_pos_is_normalized() {
        assert_eq!(
            split_translation("vt.& vi. 推，推动"),
            [
                sense(Some("vt. & vi."), None, "推"),
                sense(Some("vt. & vi."), None, "推动"),
            ]
        );
        for raw in ["n./v. 推", "n.,v. 推", "n. , v. 推", "n. &v. 推"] {
            assert_eq!(
                split_translation(raw)[0].pos.as_deref(),
                Some("n. & v."),
                "{raw}"
            );
        }
        assert_eq!(
            split_translation("suff. 表示性质")[0].pos.as_deref(),
            Some("suff.")
        );
    }

    #[test]
    fn unknown_or_glued_prefixes_are_text() {
        assert_eq!(
            split_translation("etc. 等等")[0],
            sense(None, None, "etc. 等等")
        );
        assert_eq!(
            split_translation("a.m. 上午")[0],
            sense(None, None, "a.m. 上午")
        );
        assert_eq!(
            split_translation("n.(Gate)人名")[0],
            sense(None, None, "n.(Gate)人名")
        );
    }

    #[test]
    fn brackets_and_digit_commas_do_not_split() {
        assert_eq!(
            texts("n. 导弹( missile的复数形式 ); 投射物"),
            ["导弹( missile的复数形式 )", "投射物"]
        );
        assert_eq!(
            texts("v. 使(马,鹰等)戴头罩，给…加罩"),
            ["使(马,鹰等)戴头罩", "给…加罩"]
        );
        assert_eq!(texts("n. 三千（3,000）元, 钱"), ["三千（3,000）元", "钱"]);
        assert_eq!(texts("num. 3,000"), ["3,000"]);
    }

    #[test]
    fn empty_parts_and_leading_spaces_are_dropped() {
        assert_eq!(
            texts(" 期望, 盼望; 瞩望; 属望; 企"),
            ["期望, 盼望", "瞩望", "属望", "企"]
        );
        assert_eq!(texts("n. 甲；；乙；"), ["甲", "乙"]);
        assert!(texts("").is_empty());
    }

    #[test]
    fn real_newlines_also_separate_lines() {
        assert_eq!(texts("n. 甲\nv. 乙"), ["甲", "乙"]);
    }
}
