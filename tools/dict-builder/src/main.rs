//! Builds the LexTrail compact dictionary from the pinned ECDICT source (D038).
//!
//! ```text
//! cargo run -p lextrail-dict-builder --release -- [--offline] [--raw-dir DIR] [--out FILE]
//! ```
//!
//! Defaults: the source is downloaded to `data/raw/` and the dictionary written to
//! `data/build/lextrail-dict-core.db` (both git-ignored). `--offline` uses only an already
//! downloaded source file. Format and rules: `docs/specs/m1-dict.md`.

mod build;
mod row;
mod source;

use std::fs::{self, File};
use std::io::BufReader;
use std::path::{Path, PathBuf};
use std::time::Instant;

use anyhow::{Context, Result, bail};

struct Args {
    offline: bool,
    raw_dir: PathBuf,
    out: PathBuf,
}

fn parse_args() -> Result<Args> {
    // tools/dict-builder -> repository root.
    let root = Path::new(env!("CARGO_MANIFEST_DIR"))
        .ancestors()
        .nth(2)
        .context("unexpected crate location")?
        .to_owned();
    let mut args = Args {
        offline: false,
        raw_dir: root.join("data").join("raw"),
        out: root
            .join("data")
            .join("build")
            .join(lextrail_dict::CORE_DICT_FILE_NAME),
    };
    let mut it = std::env::args().skip(1);
    while let Some(arg) = it.next() {
        match arg.as_str() {
            "--offline" => args.offline = true,
            "--raw-dir" => args.raw_dir = it.next().context("--raw-dir needs a value")?.into(),
            "--out" => args.out = it.next().context("--out needs a value")?.into(),
            "-h" | "--help" => {
                println!("usage: lextrail-dict-builder [--offline] [--raw-dir DIR] [--out FILE]");
                std::process::exit(0);
            }
            other => bail!("unknown argument {other:?} (see --help)"),
        }
    }
    Ok(args)
}

fn main() -> Result<()> {
    let args = parse_args()?;
    let src = &source::ECDICT;
    let csv = source::ensure(src, &args.raw_dir, args.offline)?;
    eprintln!("source: {} (verified)", csv.display());

    if let Some(dir) = args.out.parent() {
        fs::create_dir_all(dir).with_context(|| format!("creating {}", dir.display()))?;
    }
    let started = Instant::now();
    let provenance = build::Provenance {
        url: src.url,
        commit: src.commit,
        sha256: src.sha256,
    };
    let stats = build::build(BufReader::new(File::open(&csv)?), &args.out, &provenance)?;
    let size = fs::metadata(&args.out)?.len();
    eprintln!(
        "wrote {} in {:.1}s: {} of {} rows, {} senses, {} forms, {} dropped with no senses, \
         {} extra words; {:.1} MiB, SHA-256 {}",
        args.out.display(),
        started.elapsed().as_secs_f64(),
        stats.entries,
        stats.rows,
        stats.senses,
        stats.forms,
        stats.empty,
        stats.extra_words,
        size as f64 / (1024.0 * 1024.0),
        source::sha256_file(&args.out)?,
    );
    Ok(())
}
