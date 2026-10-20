//! ECDICT's `exchange` field: word forms, e.g. `p:pushed/i:pushing/d:pushed/3:pushes`.

/// Kind of a word form, keyed by ECDICT's one-character code.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum FormKind {
    /// `p`: past tense.
    Past,
    /// `d`: past participle.
    PastParticiple,
    /// `i`: present participle.
    PresentParticiple,
    /// `3`: third person singular.
    ThirdPerson,
    /// `s`: plural.
    Plural,
    /// `r`: comparative.
    Comparative,
    /// `t`: superlative.
    Superlative,
}

impl FormKind {
    pub fn from_code(code: &str) -> Option<Self> {
        Some(match code {
            "p" => Self::Past,
            "d" => Self::PastParticiple,
            "i" => Self::PresentParticiple,
            "3" => Self::ThirdPerson,
            "s" => Self::Plural,
            "r" => Self::Comparative,
            "t" => Self::Superlative,
            _ => return None,
        })
    }

    /// Chinese name for display: 过去式, 复数, ...
    pub fn name_zh(self) -> &'static str {
        match self {
            Self::Past => "过去式",
            Self::PastParticiple => "过去分词",
            Self::PresentParticiple => "现在分词",
            Self::ThirdPerson => "第三人称单数",
            Self::Plural => "复数",
            Self::Comparative => "比较级",
            Self::Superlative => "最高级",
        }
    }

    /// The ECDICT code, also used in the dictionary file (`forms.kind`).
    pub fn code(self) -> &'static str {
        match self {
            Self::Past => "p",
            Self::PastParticiple => "d",
            Self::PresentParticiple => "i",
            Self::ThirdPerson => "3",
            Self::Plural => "s",
            Self::Comparative => "r",
            Self::Superlative => "t",
        }
    }
}

/// A parsed `exchange` value.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct Exchange {
    /// Forms of this word (`p:`, `d:`, ... entries).
    pub forms: Vec<(FormKind, String)>,
    /// `0:`: this word is itself a form of `lemma`.
    pub lemma: Option<String>,
    /// `1:`: which kinds of form this word is of its lemma (`1:pd` = past and past participle).
    pub lemma_kinds: Vec<FormKind>,
}

/// Parses an `exchange` value. Unknown codes and malformed items are skipped.
pub fn parse_exchange(raw: &str) -> Exchange {
    let mut out = Exchange::default();
    for item in raw.split('/') {
        let Some((code, value)) = item.split_once(':') else {
            continue;
        };
        let value = value.trim();
        if value.is_empty() {
            continue;
        }
        match code.trim() {
            "0" => out.lemma = Some(value.to_owned()),
            "1" => {
                out.lemma_kinds = value
                    .chars()
                    .filter_map(|c| FormKind::from_code(c.encode_utf8(&mut [0; 4])))
                    .collect();
            }
            code => {
                if let Some(kind) = FormKind::from_code(code) {
                    out.forms.push((kind, value.to_owned()));
                }
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_forms() {
        let ex = parse_exchange("p:pushed/i:pushing/d:pushed/3:pushes");
        assert_eq!(
            ex.forms,
            [
                (FormKind::Past, "pushed".to_owned()),
                (FormKind::PresentParticiple, "pushing".to_owned()),
                (FormKind::PastParticiple, "pushed".to_owned()),
                (FormKind::ThirdPerson, "pushes".to_owned()),
            ]
        );
        assert_eq!(ex.lemma, None);
    }

    #[test]
    fn parses_lemma_of_a_form() {
        let ex = parse_exchange("0:launch/1:pd");
        assert_eq!(ex.lemma.as_deref(), Some("launch"));
        assert_eq!(ex.lemma_kinds, [FormKind::Past, FormKind::PastParticiple]);
        assert!(ex.forms.is_empty());
    }

    #[test]
    fn skips_unknown_and_malformed_items() {
        let ex = parse_exchange("s:clutters/f:x/b:y/junk/r:/:z/ t : better ");
        assert_eq!(
            ex.forms,
            [
                (FormKind::Plural, "clutters".to_owned()),
                (FormKind::Superlative, "better".to_owned()),
            ]
        );
        assert_eq!(parse_exchange(""), Exchange::default());
    }

    #[test]
    fn codes_round_trip() {
        for code in ["p", "d", "i", "3", "s", "r", "t"] {
            assert_eq!(FormKind::from_code(code).map(FormKind::code), Some(code));
        }
    }
}
