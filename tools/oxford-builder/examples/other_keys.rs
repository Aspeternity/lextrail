//! Development aid: where the keys whose Oxford relations are all `other` end up when they
//! are looked up as "not in Oxford" (`docs/specs/m1.5-oxford.md` 6.2 step 3): ECDICT's own
//! entry, an ECDICT base form that Oxford has, an ECDICT base form only ECDICT has, or
//! nothing (only then `Related`). Prints counts and a few keys of each kind.
//! `cargo run -p lextrail-oxford-builder --release --example other_keys -- <oalecd10.db> <ECDICT .db>`

use std::collections::BTreeMap;

use lextrail_dict::{Dictionary, EntryRole};
use rusqlite::{Connection, OpenFlags};

fn main() -> anyhow::Result<()> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let [oxford, ecdict] = args.as_slice() else {
        anyhow::bail!("usage: other_keys <oalecd10.db> <ECDICT .db>");
    };
    let conn = Connection::open_with_flags(oxford, OpenFlags::SQLITE_OPEN_READ_ONLY)?;
    let dict = Dictionary::open(std::path::Path::new(ecdict))?;
    let keys: Vec<String> = conn
        .prepare(
            "SELECT min(key) FROM keys GROUP BY key COLLATE NOCASE
             HAVING sum(relation <> 'other') = 0 ORDER BY 1",
        )?
        .query_map([], |r| r.get(0))?
        .collect::<Result<_, _>>()?;
    let mut headword =
        conn.prepare("SELECT 1 FROM keys WHERE key = ?1 COLLATE NOCASE AND relation = 'headword'")?;
    let mut kinds: BTreeMap<&str, Vec<String>> = BTreeMap::new();
    for key in &keys {
        let kind = match dict.lookup(key)? {
            None => "related only",
            Some(found) => {
                let own = found
                    .entries
                    .iter()
                    .find(|f| matches!(f.role, EntryRole::Query | EntryRole::FormOf(_)));
                let lemma = found.inflection.as_ref().map(|i| i.lemma.clone());
                match (own, lemma) {
                    (Some(_), Some(l)) if headword.exists([&l])? => "own entry + Oxford lemma",
                    (Some(_), Some(_)) => "own entry + ECDICT lemma",
                    (Some(f), None) if is_note(&f.entry) => "own entry (form note only)",
                    (Some(_), None) => "own entry",
                    (None, Some(l)) if headword.exists([&l])? => "lemma in Oxford",
                    (None, Some(_)) => "lemma in ECDICT only",
                    (None, None) => unreachable!("a result has an entry"),
                }
            }
        };
        kinds.entry(kind).or_default().push(key.clone());
    }
    println!("keys: {}", keys.len());
    for (kind, keys) in &kinds {
        let sample: Vec<&str> = keys.iter().take(8).map(String::as_str).collect();
        println!("{kind}: {} ({})", keys.len(), sample.join(", "));
    }
    Ok(())
}

/// Every sense says only "past tense of ..." and the like (M1's `is_form_note`).
fn is_note(entry: &lextrail_dict::Entry) -> bool {
    const NOTES: [&str; 7] = [
        "的过去式",
        "的过去分词",
        "的现在分词",
        "的复数",
        "的第三人称单数",
        "的比较级",
        "的最高级",
    ];
    !entry.senses.is_empty()
        && entry
            .senses
            .iter()
            .all(|s| NOTES.iter().any(|n| s.text.contains(n)))
}
