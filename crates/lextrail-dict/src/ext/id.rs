//! Stable ids of the senses and groups (idioms, phrasal verbs, derivatives) of an extended
//! dictionary (D052, `docs/specs/m1.5-oxford.md` section 5).
//!
//! An id is `<source>/<entry key>/<group>[/<sense>]`, e.g. `oalecd10/blorft||noun/s/2` or
//! `oalecd10/blorft|2|verb/i1`. Groups and senses are numbered by the structure of the
//! source entry (the n-th idiom element, the n-th sense element in its group), never by
//! what the converter keeps or shows, so changing the conversion rules keeps the ids.

use std::fmt;

/// Identifies an entry by its content: headword, homograph number and parts of speech.
/// The few entries that share all three get `dup` 2, 3, ... in source order.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct EntryKey {
    /// The headword as written in the entry, without syllable dots; case kept.
    pub headword: String,
    /// The homograph number, empty when the entry has none.
    pub homograph: String,
    /// All parts of speech of the entry's head block, in order; empty when it has none.
    pub pos: Vec<String>,
    /// 1 for the first entry with this headword, homograph number and parts of speech.
    pub dup: u32,
}

/// A group of senses within an entry.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Group {
    /// The entry's main senses.
    Main,
    /// The n-th idiom (from 1), counting every idiom element in the entry in order.
    Idiom(u32),
    /// The n-th phrasal verb (from 1).
    PhrasalVerb(u32),
    /// The n-th derivative (from 1).
    Derivative(u32),
}

/// A group of an entry: an idiom, phrasal verb or derivative (or the main senses).
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct GroupId {
    pub source: String,
    pub entry: EntryKey,
    pub group: Group,
}

/// A sense: the n-th sense element (from 1) of its group.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct SenseId {
    pub group: GroupId,
    pub sense: u32,
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("not a sense or group id: {0:?}")]
pub struct IdError(String);

/// Characters that separate the parts of an id; escaped inside the parts.
fn escape(part: &str, out: &mut String) {
    for c in part.chars() {
        match c {
            '%' => out.push_str("%25"),
            '/' => out.push_str("%2F"),
            '|' => out.push_str("%7C"),
            ',' => out.push_str("%2C"),
            c => out.push(c),
        }
    }
}

fn unescape(part: &str) -> Option<String> {
    let mut out = String::with_capacity(part.len());
    let mut rest = part;
    while let Some(i) = rest.find('%') {
        out.push_str(&rest[..i]);
        let code = rest.get(i + 1..i + 3)?;
        out.push(match code {
            "25" => '%',
            "2F" => '/',
            "7C" => '|',
            "2C" => ',',
            _ => return None,
        });
        rest = &rest[i + 3..];
    }
    out.push_str(rest);
    Some(out)
}

impl fmt::Display for EntryKey {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let mut s = String::new();
        escape(&self.headword, &mut s);
        s.push('|');
        escape(&self.homograph, &mut s);
        s.push('|');
        for (i, pos) in self.pos.iter().enumerate() {
            if i > 0 {
                s.push(',');
            }
            escape(pos, &mut s);
        }
        if self.dup > 1 {
            s.push('|');
            s.push_str(&self.dup.to_string());
        }
        f.write_str(&s)
    }
}

impl EntryKey {
    fn parse(text: &str) -> Option<Self> {
        let parts: Vec<&str> = text.split('|').collect();
        let (headword, homograph, pos, dup) = match parts.as_slice() {
            [h, hm, pos] => (h, hm, pos, 1),
            [h, hm, pos, dup] => {
                let dup: u32 = dup.parse().ok().filter(|&d| d > 1)?;
                (h, hm, pos, dup)
            }
            _ => return None,
        };
        let pos = if pos.is_empty() {
            Vec::new()
        } else {
            pos.split(',').map(unescape).collect::<Option<Vec<_>>>()?
        };
        Some(Self {
            headword: unescape(headword).filter(|h| !h.is_empty())?,
            homograph: unescape(homograph)?,
            pos,
            dup,
        })
    }
}

impl fmt::Display for Group {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Main => f.write_str("s"),
            Self::Idiom(n) => write!(f, "i{n}"),
            Self::PhrasalVerb(n) => write!(f, "p{n}"),
            Self::Derivative(n) => write!(f, "d{n}"),
        }
    }
}

impl Group {
    fn parse(text: &str) -> Option<Self> {
        if text == "s" {
            return Some(Self::Main);
        }
        let (kind, n) = text.split_at_checked(1)?;
        let n: u32 = n.parse().ok().filter(|&n| n > 0)?;
        if n.to_string() != text[1..] {
            return None;
        }
        match kind {
            "i" => Some(Self::Idiom(n)),
            "p" => Some(Self::PhrasalVerb(n)),
            "d" => Some(Self::Derivative(n)),
            _ => None,
        }
    }
}

impl fmt::Display for GroupId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let mut source = String::new();
        escape(&self.source, &mut source);
        write!(f, "{source}/{}/{}", self.entry, self.group)
    }
}

impl fmt::Display for SenseId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}/{}", self.group, self.sense)
    }
}

impl std::str::FromStr for GroupId {
    type Err = IdError;

    fn from_str(text: &str) -> Result<Self, IdError> {
        let err = || IdError(text.to_owned());
        let parts: Vec<&str> = text.split('/').collect();
        let [source, entry, group] = parts.as_slice() else {
            return Err(err());
        };
        Ok(Self {
            source: unescape(source).filter(|s| !s.is_empty()).ok_or_else(err)?,
            entry: EntryKey::parse(entry).ok_or_else(err)?,
            group: Group::parse(group).ok_or_else(err)?,
        })
    }
}

impl std::str::FromStr for SenseId {
    type Err = IdError;

    fn from_str(text: &str) -> Result<Self, IdError> {
        let err = || IdError(text.to_owned());
        let (group, sense) = text.rsplit_once('/').ok_or_else(err)?;
        let sense: u32 = sense.parse().ok().filter(|&n| n > 0).ok_or_else(err)?;
        if sense.to_string() != text[group.len() + 1..] {
            return Err(err());
        }
        Ok(Self {
            group: group.parse()?,
            sense,
        })
    }
}

/// Numbers the groups of one entry in source order: call [`GroupCounter::next`] for each
/// idiom, phrasal verb and derivative element as it appears, whether or not it is kept.
#[derive(Debug, Default)]
pub struct GroupCounter {
    idioms: u32,
    phrasal_verbs: u32,
    derivatives: u32,
}

/// The kind of a group element, for [`GroupCounter`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GroupKind {
    Idiom,
    PhrasalVerb,
    Derivative,
}

impl GroupCounter {
    pub fn next(&mut self, kind: GroupKind) -> Group {
        match kind {
            GroupKind::Idiom => {
                self.idioms += 1;
                Group::Idiom(self.idioms)
            }
            GroupKind::PhrasalVerb => {
                self.phrasal_verbs += 1;
                Group::PhrasalVerb(self.phrasal_verbs)
            }
            GroupKind::Derivative => {
                self.derivatives += 1;
                Group::Derivative(self.derivatives)
            }
        }
    }
}

/// Gives `dup` numbers to entry keys in source order: the first entry with a headword,
/// homograph number and parts of speech gets 1, the next one with the same three gets 2.
#[derive(Debug, Default)]
pub struct DupCounter {
    seen: std::collections::HashMap<(String, String, Vec<String>), u32>,
}

impl DupCounter {
    pub fn key(&mut self, headword: &str, homograph: &str, pos: &[String]) -> EntryKey {
        let n = self
            .seen
            .entry((headword.to_owned(), homograph.to_owned(), pos.to_vec()))
            .or_insert(0);
        *n += 1;
        EntryKey {
            headword: headword.to_owned(),
            homograph: homograph.to_owned(),
            pos: pos.to_vec(),
            dup: *n,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn key(h: &str, hm: &str, pos: &[&str], dup: u32) -> EntryKey {
        EntryKey {
            headword: h.into(),
            homograph: hm.into(),
            pos: pos.iter().map(|p| p.to_string()).collect(),
            dup,
        }
    }

    fn sense(entry: EntryKey, group: Group, sense: u32) -> SenseId {
        SenseId {
            group: GroupId {
                source: "oalecd10".into(),
                entry,
                group,
            },
            sense,
        }
    }

    #[test]
    fn formats() {
        let id = sense(key("blorft", "", &["noun"], 1), Group::Main, 2);
        assert_eq!(id.to_string(), "oalecd10/blorft||noun/s/2");
        let id = sense(key("blorft", "2", &["verb"], 1), Group::Idiom(1), 1);
        assert_eq!(id.to_string(), "oalecd10/blorft|2|verb/i1/1");
        let id = sense(
            key("Blorft", "", &["noun", "verb"], 2),
            Group::PhrasalVerb(3),
            4,
        );
        assert_eq!(id.to_string(), "oalecd10/Blorft||noun,verb|2/p3/4");
        let id = sense(key("snib", "", &[], 1), Group::Derivative(2), 1);
        assert_eq!(id.to_string(), "oalecd10/snib||/d2/1");
        assert_eq!(id.group.to_string(), "oalecd10/snib||/d2");
    }

    #[test]
    fn separators_in_parts_are_escaped() {
        let id = sense(key("a/b|c%d,e", "", &["x,y"], 1), Group::Main, 1);
        assert_eq!(id.to_string(), "oalecd10/a%2Fb%7Cc%25d%2Ce||x%2Cy/s/1");
        assert_eq!(id.to_string().parse::<SenseId>().unwrap(), id);
    }

    #[test]
    fn round_trips() {
        for text in [
            "oalecd10/blorft||noun/s/2",
            "oalecd10/blorft|2|verb/i1/1",
            "oalecd10/Blorft||noun,verb|2/p3/4",
            "oalecd10/snib||/d2/1",
            "oalecd10/blorft out||/s/12",
        ] {
            assert_eq!(text.parse::<SenseId>().unwrap().to_string(), text);
        }
        let g: GroupId = "oalecd10/snib||/d2".parse().unwrap();
        assert_eq!(g.group, Group::Derivative(2));
    }

    #[test]
    fn rejects_malformed_ids() {
        for text in [
            "",
            "oalecd10/blorft||noun/s",
            "oalecd10/blorft||noun/s/0",
            "oalecd10/blorft||noun/s/02",
            "oalecd10/blorft|noun/s/1",
            "oalecd10/blorft||noun|1/s/1",
            "oalecd10/||noun/s/1",
            "oalecd10/blorft||noun/x1/1",
            "oalecd10/blorft||noun/i0/1",
            "oalecd10/blorft||noun/i01/1",
            "oalecd10/blo%2rft||noun/s/1",
            "/blorft||noun/s/1",
        ] {
            assert!(text.parse::<SenseId>().is_err(), "{text}");
        }
    }

    #[test]
    fn groups_are_counted_per_kind_in_source_order() {
        let mut c = GroupCounter::default();
        assert_eq!(c.next(GroupKind::Idiom), Group::Idiom(1));
        assert_eq!(c.next(GroupKind::Derivative), Group::Derivative(1));
        assert_eq!(c.next(GroupKind::Idiom), Group::Idiom(2));
        assert_eq!(c.next(GroupKind::PhrasalVerb), Group::PhrasalVerb(1));
    }

    #[test]
    fn duplicate_entry_keys_get_numbers_in_source_order() {
        let mut d = DupCounter::default();
        let noun = vec!["noun".to_owned()];
        assert_eq!(d.key("blorft", "", &noun).dup, 1);
        assert_eq!(d.key("blorft", "", &["verb".to_owned()]).dup, 1);
        assert_eq!(d.key("blorft", "", &noun).dup, 2);
        assert_eq!(d.key("Blorft", "", &noun).dup, 1);
    }
}
