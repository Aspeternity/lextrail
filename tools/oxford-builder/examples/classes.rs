//! Development aid: counts each class name (once per entry, and in total) with its most
//! common parent class, over the whole MDX. Prints structure only, no text.
//! `cargo run -p lextrail-oxford-builder --release --example classes -- <mdx>`

use std::collections::{BTreeMap, HashMap, HashSet};

use scraper::{ElementRef, Html};

fn main() -> anyhow::Result<()> {
    let mdx = std::env::args().nth(1).expect("usage: classes <mdx>");
    let file = mdictlib::MdxFile::open(mdx)?;
    let started = std::time::Instant::now();
    let mut seen = HashSet::new();
    let mut total: BTreeMap<String, (u64, u64)> = BTreeMap::new();
    let mut parents: HashMap<String, HashMap<String, u64>> = HashMap::new();
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
        let mut here = HashSet::new();
        for node in doc.root_element().descendants() {
            let Some(el) = ElementRef::wrap(node) else {
                continue;
            };
            for class in el.value().classes() {
                let t = total.entry(class.to_owned()).or_default();
                t.1 += 1;
                if here.insert(class.to_owned()) {
                    t.0 += 1;
                }
                let parent = el
                    .ancestors()
                    .filter_map(ElementRef::wrap)
                    .find_map(|p| p.value().classes().next().map(str::to_owned))
                    .unwrap_or_else(|| "-".into());
                *parents
                    .entry(class.to_owned())
                    .or_default()
                    .entry(parent)
                    .or_default() += 1;
            }
        }
    }
    for (class, (entries, count)) in &total {
        let mut ps: Vec<_> = parents[class].iter().collect();
        ps.sort_by(|a, b| b.1.cmp(a.1));
        let ps: Vec<String> = ps.iter().take(3).map(|(p, n)| format!("{p}:{n}")).collect();
        println!("{class}\t{entries}\t{count}\t{}", ps.join(" "));
    }
    eprintln!(
        "{} entries in {:.1}s",
        seen.len(),
        started.elapsed().as_secs_f64()
    );
    Ok(())
}
