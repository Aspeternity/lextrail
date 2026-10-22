//! Converts the Oxford Advanced Learner's English-Chinese Dictionary, 10th edition, to a
//! LexTrail extended dictionary (D049, `docs/specs/m1.5-oxford.md` section 3).
//!
//! ```text
//! cargo run -p lextrail-oxford-builder --release -- --mdx <.mdx> --mdd <.mdd> --mdd <.1.mdd>
//!     --ecdict data/build/lextrail-dict-core.db --out <folder> [--previous <old oalecd10.db>]
//!     [--report <report.json>]
//! ```

use std::path::PathBuf;

use anyhow::{Context, Result, bail};
use lextrail_oxford_builder::{Args, Report, interrupt, now_utc, run};

const USAGE: &str = "usage: lextrail-oxford-builder --mdx FILE --mdd FILE [--mdd FILE ...] \
                     --ecdict FILE --out DIR [--previous FILE] [--report FILE]";

fn parse_args() -> Result<(Args, Option<PathBuf>)> {
    let mut mdx = None;
    let mut mdds = Vec::new();
    let mut ecdict = None;
    let mut out = None;
    let mut previous = None;
    let mut report = None;
    let mut it = std::env::args().skip(1);
    while let Some(arg) = it.next() {
        let mut value = || it.next().with_context(|| format!("{arg} needs a value"));
        match arg.as_str() {
            "--mdx" => mdx = Some(PathBuf::from(value()?)),
            "--mdd" => mdds.push(PathBuf::from(value()?)),
            "--ecdict" => ecdict = Some(PathBuf::from(value()?)),
            "--out" => out = Some(PathBuf::from(value()?)),
            "--previous" => previous = Some(PathBuf::from(value()?)),
            "--report" => report = Some(PathBuf::from(value()?)),
            "-h" | "--help" => {
                println!("{USAGE}");
                std::process::exit(0);
            }
            other => bail!("unknown argument {other:?}\n{USAGE}"),
        }
    }
    let args = Args {
        mdx: mdx.context(USAGE)?,
        mdds,
        ecdict: ecdict.context("--ecdict is required (the relation rules use its forms)")?,
        out: out.context(USAGE)?,
        previous,
    };
    if args.mdds.is_empty() {
        bail!("give the .mdd files with --mdd\n{USAGE}");
    }
    Ok((args, report))
}

fn main() -> Result<()> {
    let (args, report_path) = parse_args()?;
    std::fs::create_dir_all(&args.out)
        .with_context(|| format!("creating {}", args.out.display()))?;
    let out = std::path::absolute(&args.out)?;
    // SQLite's own temporary files (index sorting) go to the output folder, not %TEMP%.
    // SAFETY: no other threads exist yet (the Ctrl+C handler thread is started below).
    unsafe {
        std::env::set_var("TMP", &out);
        std::env::set_var("TEMP", &out);
        std::env::set_var("SQLITE_TMPDIR", &out);
    }
    ctrlc::set_handler(interrupt).context("installing the Ctrl+C handler")?;

    let report = run(&args, &now_utc())?;
    print_report(&report);
    if let Some(path) = report_path {
        std::fs::write(&path, serde_json::to_string_pretty(&report)?)
            .with_context(|| format!("writing {}", path.display()))?;
    }
    Ok(())
}

fn print_report(r: &Report) {
    let c = &r.counts;
    eprintln!(
        "records {} ({} pages skipped); entries {} (duplicate entry keys {}); keys {} \
         (dropped empty {})",
        r.records, r.pages_skipped, c.entries, c.duplicate_entry_keys, c.keys, c.keys_dropped_empty
    );
    eprintln!("relations {:?}", c.relations);
    eprintln!("groups {:?}", c.groups);
    eprintln!(
        "senses {} (alt fingerprint {}, repeated fingerprint in group {}); examples {}; \
         forms {}; variants {}; notes {}",
        c.senses,
        c.senses_alt_fingerprint,
        c.senses_duplicate_fingerprint_in_group,
        c.examples,
        c.forms,
        c.variants,
        c.notes
    );
    eprintln!(
        "coverage: BrE {} NAmE {} CEFR {} Oxford 3000 {} 5000 {} OPAL {}",
        c.with_phon_gb, c.with_phon_us, c.with_cefr, c.oxford_3000, c.oxford_5000, c.with_opal
    );
    eprintln!("parse {:?}", r.parse);
    eprintln!(
        "audio: {} referenced, {} copied ({:.1} MiB), {} missing",
        r.audio.referenced,
        r.audio.copied,
        r.audio.bytes as f64 / 1048576.0,
        r.audio.missing
    );
    if let Some(n) = &r.renames {
        eprintln!("renames {n:?}");
    }
    eprintln!("done in {:.1}s", r.seconds);
}
