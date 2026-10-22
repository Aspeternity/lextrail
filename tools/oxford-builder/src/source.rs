//! Reads the MDX: records grouped by entry number, each entry parsed once
//! (`docs/specs/m1.5-oxford.md` 3, step 1–2).

use std::collections::BTreeMap;
use std::path::Path;

use anyhow::{Context, Result, bail};
use sha2::{Digest, Sha256};

use crate::parse::{Entry, ParseStats, parse};

/// An entry with its lookup keys (as written in the MDX, in record order).
pub struct SourceEntry {
    pub entry: Entry,
    pub keys: Vec<String>,
}

pub struct Source {
    /// By `data-hkoup-entry`, ascending.
    pub entries: Vec<SourceEntry>,
    pub records: u64,
    /// Records without an entry number (help and topic pages), skipped.
    pub pages: u64,
    pub stats: ParseStats,
}

fn entry_number(html: &str) -> Option<u32> {
    const ATTR: &str = "data-hkoup-entry=\"";
    let start = html.find(ATTR)? + ATTR.len();
    let digits: String = html[start..]
        .chars()
        .take_while(char::is_ascii_digit)
        .collect();
    digits.parse().ok()
}

/// Reads every record of `mdx`. All records of one entry must be byte-identical (0.7 b);
/// if they are not, the source differs from the one the rules were written for, so stop.
pub fn read(mdx: &Path, check_interrupted: &dyn Fn() -> Result<()>) -> Result<Source> {
    let file =
        mdictlib::MdxFile::open(mdx).with_context(|| format!("opening {}", mdx.display()))?;
    let mut by_number: BTreeMap<u32, ([u8; 32], SourceEntry)> = BTreeMap::new();
    let mut source = Source {
        entries: Vec::new(),
        records: 0,
        pages: 0,
        stats: ParseStats::default(),
    };
    for record in file.entries() {
        let record = record.with_context(|| format!("reading {}", mdx.display()))?;
        source.records += 1;
        if source.records.is_multiple_of(4096) {
            check_interrupted()?;
        }
        let html = record.text();
        let Some(number) = entry_number(html) else {
            source.pages += 1;
            continue;
        };
        let hash: [u8; 32] = Sha256::digest(html.as_bytes()).into();
        match by_number.get_mut(&number) {
            Some((seen, e)) => {
                if *seen != hash {
                    bail!(
                        "entry {number}: the records under {:?} and {:?} differ; the source is \
                         not the export the conversion rules were written for",
                        e.keys[0],
                        record.key()
                    );
                }
                if !e.keys.iter().any(|k| k == record.key()) {
                    e.keys.push(record.key().to_owned());
                }
            }
            None => {
                let entry = parse(html, number, &mut source.stats)?;
                by_number.insert(
                    number,
                    (
                        hash,
                        SourceEntry {
                            entry,
                            keys: vec![record.key().to_owned()],
                        },
                    ),
                );
            }
        }
    }
    source.entries = by_number.into_values().map(|(_, e)| e).collect();
    Ok(source)
}
