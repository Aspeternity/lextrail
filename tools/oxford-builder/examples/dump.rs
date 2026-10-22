//! Development aid: writes the HTML of the entries found under each key to `<out>/<key>.html`.
//! `cargo run -p lextrail-oxford-builder --example dump -- <mdx> <out dir> <key>...`
//! A key `grep:<text>` writes the first three entries whose HTML contains `<text>`.

fn main() -> anyhow::Result<()> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let [mdx, out, keys @ ..] = args.as_slice() else {
        anyhow::bail!("usage: dump <mdx> <out dir> <key>...");
    };
    let file = mdictlib::MdxFile::open(mdx)?;
    std::fs::create_dir_all(out)?;
    for key in keys {
        if let Some(needle) = key.strip_prefix("grep:") {
            let mut html = String::new();
            let mut found = 0;
            for entry in file.entries() {
                let entry = entry?;
                if entry.text().contains(needle) && !html.contains(entry.text()) {
                    html.push_str(&format!(
                        "<!-- key {:?} -->
{}
",
                        entry.key(),
                        entry.text()
                    ));
                    found += 1;
                    if found == 3 {
                        break;
                    }
                }
            }
            let name: String = needle
                .chars()
                .filter(|c| c.is_ascii_alphanumeric() || *c == '-')
                .collect();
            std::fs::write(
                std::path::Path::new(out).join(format!("grep-{name}.html")),
                html,
            )?;
            continue;
        }
        let Some(matches) = file.locate(key)? else {
            eprintln!("{key}: not found");
            continue;
        };
        let mut html = String::new();
        for ordinal in matches.iter() {
            if let Some(entry) = file.entry_at(ordinal)? {
                html.push_str(&format!("<!-- key {:?} -->\n", entry.key()));
                html.push_str(entry.text());
                html.push('\n');
            }
        }
        std::fs::write(std::path::Path::new(out).join(format!("{key}.html")), html)?;
    }
    Ok(())
}
