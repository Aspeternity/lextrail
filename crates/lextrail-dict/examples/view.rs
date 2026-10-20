//! Prints what the popup would show for each text, as JSON (`lextrail_dict::view`).
//!
//! ```text
//! cargo run -p lextrail-dict --example view -- <dictionary.db> [--start] <text>...
//! ```
//!
//! `--start`: the texts start a sentence (a capitalized word lists its lowercase entry
//! first).

use std::path::Path;

use lextrail_dict::{Dictionary, LookupOptions};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut args = std::env::args().skip(1);
    let dict = args
        .next()
        .ok_or("usage: view <dictionary.db> [--start] <text>...")?;
    let dict = Dictionary::open(Path::new(&dict))?;
    let mut options = LookupOptions::default();
    for text in args {
        if text == "--start" {
            options.sentence_start = true;
            continue;
        }
        let view = dict.view(&text, options)?;
        println!("{text}: {}", serde_json::to_string_pretty(&view)?);
    }
    Ok(())
}
