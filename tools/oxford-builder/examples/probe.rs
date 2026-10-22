//! Development aid: distinct texts (with counts) of a few short label elements, and the
//! first resource names of each MDD. Prints labels only, no definitions or examples.
//! `cargo run -p lextrail-oxford-builder --release --example probe -- <mdx> [<mdd>...]`

use std::collections::{BTreeMap, HashSet};

use scraper::{Html, Selector};

fn main() -> anyhow::Result<()> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let file = mdictlib::MdxFile::open(&args[0])?;
    let probes = [
        ".t-form",
        ".t-hm",
        ".t-st",
        ".t-h-l",
        ".t-pronform",
        ".t-pronfreq",
        ".t-use",
        ".t-vpform",
        ".t-xrlabel",
        ".t-vgslabel",
        "[type=opal]",
        ".t-gram",
    ];
    let selectors: Vec<Selector> = probes.iter().map(|p| Selector::parse(p).unwrap()).collect();
    let mut counts: Vec<BTreeMap<String, u64>> = vec![BTreeMap::new(); probes.len()];
    let mut seen = HashSet::new();
    for entry in file.entries() {
        let entry = entry?;
        let text = entry.text();
        let Some(i) = text.find("data-hkoup-entry=\"") else {
            continue;
        };
        let id: String = text[i + 18..]
            .chars()
            .take_while(|c| c.is_ascii_digit())
            .collect();
        if !seen.insert(id) {
            continue;
        }
        let doc = Html::parse_fragment(text);
        for (sel, count) in selectors.iter().zip(&mut counts) {
            for el in doc.select(sel) {
                let t: String = el
                    .text()
                    .collect::<String>()
                    .split_whitespace()
                    .collect::<Vec<_>>()
                    .join(" ");
                let t: String = if t.chars().count() > 30 {
                    format!("<{} chars>", t.chars().count())
                } else {
                    t
                };
                *count.entry(t).or_default() += 1;
            }
        }
    }
    for (probe, count) in probes.iter().zip(&counts) {
        let mut v: Vec<_> = count.iter().collect();
        v.sort_by(|a, b| b.1.cmp(a.1));
        println!("== {probe} ({} distinct)", v.len());
        for (t, n) in v.iter().take(25) {
            println!("  {n}\t{t}");
        }
    }
    for mdd in &args[1..] {
        let mdd = mdictlib::MddFile::open(mdd)?;
        let keys: Vec<String> = mdd
            .keys()
            .take(5)
            .map(|k| k.map(|k| k.key().to_owned()))
            .collect::<Result<_, _>>()?;
        println!("== {} resources, first: {keys:?}", mdd.len());
    }
    Ok(())
}
