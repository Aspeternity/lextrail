//! Copies the headword, form, idiom ... pronunciations from the MDD files into the audio
//! file (`docs/specs/m1.5-oxford.md` 9, plan C): the original bytes, not re-encoded.

use std::collections::BTreeSet;
use std::path::Path;

use anyhow::{Context, Result};
use lextrail_dict::ext::schema::AUDIO_SCHEMA;
use rusqlite::{Connection, params};

#[derive(Debug, Default, Clone, serde::Serialize)]
pub struct AudioCounts {
    /// Distinct pronunciation files the entries link to.
    pub referenced: u64,
    pub copied: u64,
    pub bytes: u64,
    /// Linked but in none of the MDD files.
    pub missing: u64,
}

/// The MDD resource name of a `sound://` link target: `audio/x.ogg` → `\audio\x.ogg`.
pub fn resource_name(link: &str) -> String {
    format!("\\{}", link.replace('/', "\\"))
}

/// Writes the files named in `wanted` (link targets) that the MDD files hold into `conn`
/// (a new database), in MDD order.
pub fn write(
    conn: &mut Connection,
    mdds: &[impl AsRef<Path>],
    wanted: &BTreeSet<String>,
    check_interrupted: &dyn Fn() -> Result<()>,
) -> Result<AudioCounts> {
    // Large pages: most files are 2–6 KiB, and with 4 KiB pages each would take a page or
    // more of its own (a third of the file wasted).
    conn.execute_batch(
        "PRAGMA page_size = 65536; PRAGMA journal_mode = OFF; PRAGMA synchronous = OFF;",
    )?;
    conn.execute_batch(AUDIO_SCHEMA)?;
    let by_resource: std::collections::HashMap<String, &str> = wanted
        .iter()
        .map(|w| (resource_name(w), w.as_str()))
        .collect();
    let mut counts = AudioCounts {
        referenced: wanted.len() as u64,
        ..AudioCounts::default()
    };
    let mut done = BTreeSet::new();
    let tx = conn.transaction()?;
    {
        let mut insert = tx.prepare("INSERT INTO audio (name, data) VALUES (?1, ?2)")?;
        for mdd in mdds {
            let mdd = mdd.as_ref();
            let file = mdictlib::MddFile::open(mdd)
                .with_context(|| format!("opening {}", mdd.display()))?;
            for (i, resource) in file.resources().enumerate() {
                if i.is_multiple_of(4096) {
                    check_interrupted()?;
                }
                let resource = resource.with_context(|| format!("reading {}", mdd.display()))?;
                let Some(&name) = by_resource.get(resource.key()) else {
                    continue;
                };
                if !done.insert(name) {
                    continue;
                }
                insert.execute(params![name, resource.bytes()])?;
                counts.copied += 1;
                counts.bytes += resource.bytes().len() as u64;
            }
        }
    }
    tx.commit()?;
    counts.missing = counts.referenced - counts.copied;
    Ok(counts)
}
