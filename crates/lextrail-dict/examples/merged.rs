//! Development aid: prints how merged lookups resolve (headwords, roles and relations
//! only) and how long each takes.
//! `cargo run -p lextrail-dict --release --example merged -- <oalecd10.db> <ECDICT .db> <word>...`

use std::path::Path;
use std::time::Instant;

use lextrail_dict::ext::reader::ExtDictionary;
use lextrail_dict::{Content, Dictionary, Lexicon, LookupOptions, MergedFound};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let [ext, ecdict, words @ ..] = args.as_slice() else {
        return Err("usage: merged <oalecd10.db> <ECDICT .db> <word>...".into());
    };
    let ext = ExtDictionary::open(Path::new(ext))?;
    let ecdict = Dictionary::open(Path::new(ecdict))?;
    let lex = Lexicon {
        ecdict: &ecdict,
        ext: Some(&ext),
    };
    for word in words {
        let started = Instant::now();
        let found = lex.lookup_with_parts(word, LookupOptions::default())?;
        let ms = started.elapsed().as_secs_f64() * 1000.0;
        match found {
            None => println!("{word}: not found ({ms:.2} ms)"),
            Some(MergedFound::Parts {
                parts, not_found, ..
            }) => println!(
                "{word}: parts {:?}, not found {not_found:?} ({ms:.2} ms)",
                parts.iter().map(|p| p.query.as_str()).collect::<Vec<_>>()
            ),
            Some(MergedFound::Whole(m)) => {
                println!(
                    "{word}: lemma {} notes {:?} ({ms:.2} ms)",
                    m.lemma,
                    m.notes()
                );
                for h in &m.hits {
                    let (src, what) = match &h.content {
                        Content::Ecdict(e) => ("ECDICT", e.word.clone()),
                        Content::Oxford(o) => (
                            "Oxford",
                            format!("{} {}", o.entry.headword, o.entry.pos.join(",")),
                        ),
                    };
                    println!(
                        "  {src} {what} {:?} {} {}",
                        h.role,
                        h.relation.as_deref().unwrap_or(""),
                        h.open_group.as_deref().unwrap_or("")
                    );
                }
            }
        }
    }
    Ok(())
}
