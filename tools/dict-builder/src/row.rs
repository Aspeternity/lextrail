//! ECDICT CSV rows and the compact dictionary's selection rule
//! (`docs/specs/m1-dict.md` section 3, plan B+).

use anyhow::{Context, Result, bail};
use csv::StringRecord;

/// The ECDICT columns the builder uses.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Row {
    pub word: String,
    pub phonetic: String,
    pub translation: String,
    /// Collins stars, 0 when unrated.
    pub collins: u32,
    /// Oxford 3000 core word.
    pub oxford: bool,
    /// Space-separated exam tags, e.g. `cet4 toefl`.
    pub tag: String,
    /// BNC and contemporary corpus frequency ranks, 0 when unranked.
    pub bnc: u32,
    pub frq: u32,
    pub exchange: String,
}

/// Column positions, found from the CSV header.
pub struct Columns {
    word: usize,
    phonetic: usize,
    translation: usize,
    collins: usize,
    oxford: usize,
    tag: usize,
    bnc: usize,
    frq: usize,
    exchange: usize,
}

impl Columns {
    pub fn from_header(header: &StringRecord) -> Result<Self> {
        let find = |name: &str| {
            header
                .iter()
                .position(|h| h == name)
                .with_context(|| format!("CSV header has no `{name}` column"))
        };
        Ok(Self {
            word: find("word")?,
            phonetic: find("phonetic")?,
            translation: find("translation")?,
            collins: find("collins")?,
            oxford: find("oxford")?,
            tag: find("tag")?,
            bnc: find("bnc")?,
            frq: find("frq")?,
            exchange: find("exchange")?,
        })
    }

    pub fn row(&self, record: &StringRecord) -> Result<Row> {
        let text = |i: usize| record.get(i).unwrap_or("").to_owned();
        let number = |i: usize, name: &str| -> Result<u32> {
            let raw = record.get(i).unwrap_or("").trim();
            if raw.is_empty() {
                return Ok(0);
            }
            raw.parse()
                .with_context(|| format!("`{name}` is not a number: {raw:?}"))
        };
        let oxford = match number(self.oxford, "oxford")? {
            0 => false,
            1 => true,
            n => bail!("`oxford` is {n}, expected 0 or 1"),
        };
        Ok(Row {
            word: text(self.word),
            phonetic: text(self.phonetic),
            translation: text(self.translation),
            collins: number(self.collins, "collins")?,
            oxford,
            tag: text(self.tag),
            bnc: number(self.bnc, "bnc")?,
            frq: number(self.frq, "frq")?,
            exchange: text(self.exchange),
        })
    }
}

impl Row {
    pub fn is_phrase(&self) -> bool {
        self.word.trim().contains(' ')
    }

    /// Has a frequency rank, an exam tag, Collins stars or the Oxford mark.
    fn is_marked(&self) -> bool {
        self.bnc > 0
            || self.frq > 0
            || !self.tag.trim().is_empty()
            || self.collins > 0
            || self.oxford
    }

    /// Plan B+: has a Chinese translation, and is marked, or is a single word with a
    /// phonetic, or is a phrase whose translation does not start with a domain label.
    pub fn in_core(&self) -> bool {
        let translation = self.translation.trim_start();
        if self.word.trim().is_empty() || translation.is_empty() {
            return false;
        }
        if self.is_marked() {
            return true;
        }
        if self.is_phrase() {
            !translation.starts_with('[')
        } else {
            !self.phonetic.trim().is_empty()
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn row(word: &str, phonetic: &str, translation: &str) -> Row {
        Row {
            word: word.into(),
            phonetic: phonetic.into(),
            translation: translation.into(),
            ..Row::default()
        }
    }

    #[test]
    fn marked_entries_are_in() {
        let base = row("butylbenzene", "", "[化] 丁苯");
        assert!(!base.in_core());
        assert!(
            Row {
                bnc: 10,
                ..base.clone()
            }
            .in_core()
        );
        assert!(
            Row {
                frq: 10,
                ..base.clone()
            }
            .in_core()
        );
        assert!(
            Row {
                collins: 1,
                ..base.clone()
            }
            .in_core()
        );
        assert!(
            Row {
                oxford: true,
                ..base.clone()
            }
            .in_core()
        );
        assert!(
            Row {
                tag: "gre".into(),
                ..base.clone()
            }
            .in_core()
        );
        assert!(
            !Row {
                tag: " ".into(),
                ..base
            }
            .in_core()
        );
    }

    #[test]
    fn single_words_need_a_phonetic() {
        assert!(row("transduction", "træns'dʌkʃən", "n. 转导").in_core());
        assert!(!row("butylbenzene", "", "[化] 丁苯").in_core());
    }

    #[test]
    fn phrases_need_a_general_translation() {
        assert!(row("give up", "", "放弃, 停止").in_core());
        assert!(row("look forward to", "", " 期望, 盼望").in_core());
        assert!(!row("butylcarboxylic acid", "", "[医] 戊酸, 缬草酸").in_core());
    }

    #[test]
    fn entries_without_translation_are_out() {
        assert!(
            !Row {
                bnc: 1,
                ..row("x", "eks", "  ")
            }
            .in_core()
        );
        assert!(!row(" ", "eks", "n. 叉").in_core());
    }

    #[test]
    fn reads_columns_by_header() {
        let header = StringRecord::from(vec![
            "word",
            "phonetic",
            "definition",
            "translation",
            "pos",
            "collins",
            "oxford",
            "tag",
            "bnc",
            "frq",
            "exchange",
            "detail",
            "audio",
        ]);
        let cols = Columns::from_header(&header).unwrap();
        let rec = StringRecord::from(vec![
            "push", "puʃ", "", "n. 推", "", "5", "1", "zk gk", "926", "695", "p:pushed", "", "",
        ]);
        let r = cols.row(&rec).unwrap();
        assert_eq!(
            (r.word.as_str(), r.collins, r.oxford, r.bnc, r.frq),
            ("push", 5, true, 926, 695)
        );
        assert_eq!(r.exchange, "p:pushed");

        let bad = StringRecord::from(vec![
            "x", "", "", "n. 叉", "", "", "2", "", "", "", "", "", "",
        ]);
        assert!(cols.row(&bad).is_err());
        assert!(Columns::from_header(&StringRecord::from(vec!["word"])).is_err());
    }
}
