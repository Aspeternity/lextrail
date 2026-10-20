//! Dictionary coverage of a text: which words are found, how (directly, by `exchange`
//! data, by suffix rules) and which are not.
//!
//! ```text
//! cargo run -p lextrail-dict --release --example coverage -- <dictionary.db> <text.txt>
//! ```
//!
//! Words are runs of letters (with inner `'` or `-`). A hyphen at a line end joins the two
//! halves when the joined word is in the dictionary (`trans-` / `duction`). Hyphenated
//! compounds not in the dictionary are reported apart, with whether all their parts are.

use std::collections::BTreeMap;
use std::path::Path;

use lextrail_dict::{Dictionary, InflectionSource};

#[derive(Default)]
struct Tally {
    /// Unique words per outcome, with how often each occurs.
    found: BTreeMap<&'static str, BTreeMap<String, usize>>,
    missing: BTreeMap<String, usize>,
    compounds: BTreeMap<String, (usize, bool)>,
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let [dict, text] = args.as_slice() else {
        return Err("usage: coverage <dictionary.db> <text.txt>".into());
    };
    let dict = Dictionary::open(Path::new(dict))?;
    let text = std::fs::read_to_string(text)?;
    let words = words(&join_line_end_hyphens(&dict, &text)?);

    let mut tally = Tally::default();
    for word in &words {
        let how = classify(&dict, word)?;
        match how {
            Some(how) => {
                *tally
                    .found
                    .entry(how)
                    .or_default()
                    .entry(word.clone())
                    .or_default() += 1
            }
            None if word.contains('-') => {
                let parts_found = word
                    .split('-')
                    .filter(|p| !p.is_empty())
                    .map(|p| classify(&dict, p).map(|h| h.is_some()))
                    .collect::<Result<Vec<_>, _>>()?
                    .into_iter()
                    .all(|f| f);
                tally
                    .compounds
                    .entry(word.clone())
                    .or_insert((0, parts_found))
                    .0 += 1;
            }
            None => *tally.missing.entry(word.clone()).or_default() += 1,
        }
    }

    let unique = |m: &BTreeMap<String, usize>| m.len();
    let total = |m: &BTreeMap<String, usize>| m.values().sum::<usize>();
    println!("words: {} ({} unique)", words.len(), {
        let mut u: Vec<&String> = words.iter().collect();
        u.sort();
        u.dedup();
        u.len()
    });
    for (how, m) in &tally.found {
        println!(
            "found {how}: {} unique, {} occurrences",
            unique(m),
            total(m)
        );
    }
    println!(
        "hyphenated compounds not in the dictionary: {} unique ({} with all parts found)",
        tally.compounds.len(),
        tally.compounds.values().filter(|(_, f)| *f).count()
    );
    println!(
        "missing: {} unique, {} occurrences",
        unique(&tally.missing),
        total(&tally.missing)
    );
    println!("\n## found by suffix rules (word -> base)");
    for word in tally
        .found
        .get("suffix rule")
        .into_iter()
        .flat_map(|m| m.keys())
    {
        let r = dict.lookup(word)?.expect("found above");
        println!("{word} -> {}", r.lemma());
    }
    println!("\n## hyphenated compounds (count, all parts found)");
    for (word, (n, parts)) in &tally.compounds {
        println!(
            "{word}\t{n}\t{}",
            if *parts {
                "parts found"
            } else {
                "PART MISSING"
            }
        );
    }
    println!("\n## missing (count)");
    for (word, n) in &tally.missing {
        println!("{word}\t{n}");
    }
    Ok(())
}

/// How `word` was found, or `None`.
fn classify(
    dict: &Dictionary,
    word: &str,
) -> Result<Option<&'static str>, lextrail_dict::DictError> {
    Ok(dict
        .lookup(word)?
        .map(|r| match r.inflection.map(|i| i.source) {
            None => "directly",
            Some(InflectionSource::Exchange) => "by exchange forms",
            Some(InflectionSource::SuffixRule) => "suffix rule",
            Some(InflectionSource::PhraseWords) => "phrase words",
        }))
}

/// Runs of letters, keeping inner `'` and `-`.
fn words(text: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut current = String::new();
    let chars: Vec<char> = text.chars().collect();
    for (i, &c) in chars.iter().enumerate() {
        let inner = (c == '\'' || c == '\u{2019}' || c == '-')
            && !current.is_empty()
            && chars.get(i + 1).is_some_and(|n| n.is_alphabetic());
        if c.is_alphabetic() || inner {
            current.push(if c == '\u{2019}' { '\'' } else { c });
        } else if !current.is_empty() {
            out.push(std::mem::take(&mut current));
        }
    }
    if !current.is_empty() {
        out.push(current);
    }
    // Single letters are mostly math variables.
    out.retain(|w| w.chars().count() > 1);
    out
}

/// `trans-\nduction` -> `transduction` when that is in the dictionary.
fn join_line_end_hyphens(
    dict: &Dictionary,
    text: &str,
) -> Result<String, lextrail_dict::DictError> {
    let mut out = String::with_capacity(text.len());
    let mut lines = text.lines().peekable();
    while let Some(line) = lines.next() {
        if let Some(head) = line.strip_suffix('-')
            && let Some(next) = lines.peek()
        {
            let left = head
                .rsplit(|c: char| !c.is_alphabetic())
                .next()
                .unwrap_or("");
            let right = next
                .split(|c: char| !c.is_alphabetic())
                .next()
                .unwrap_or("");
            if !left.is_empty()
                && !right.is_empty()
                && dict.lookup(&format!("{left}{right}"))?.is_some()
            {
                out.push_str(head);
                continue;
            }
        }
        out.push_str(line);
        out.push('\n');
    }
    Ok(out)
}
