//! Converts the Oxford Advanced Learner's English-Chinese Dictionary, 10th edition (MDX +
//! MDD) to a LexTrail extended dictionary (`oalecd10.db`) and its audio file
//! (`oalecd10-audio.db`). Rules: `docs/specs/m1.5-oxford.md` sections 3–6, 9.

pub mod audio;
pub mod ecdict;
pub mod html;
pub mod output;
pub mod parse;
pub mod previous;
pub mod relation;
pub mod source;
pub mod write;

use std::collections::BTreeSet;
use std::fs::File;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Instant, SystemTime, UNIX_EPOCH};

use anyhow::{Context, Result, bail};
use lextrail_dict::ext::schema::{AUDIO_FORMAT, AUDIO_FORMAT_VERSION, FORMAT, FORMAT_VERSION};
use lextrail_dict::ext::{OXFORD_AUDIO_FILE_NAME, OXFORD_FILE_NAME, OXFORD_SOURCE};
use rusqlite::{Connection, params};
use sha2::{Digest, Sha256};

use crate::output::TempFiles;

/// Version of the conversion rules, stored in `meta.converter_version`. Raise it whenever
/// the output for the same source changes.
pub const CONVERTER_VERSION: u32 = 1;

/// The source's name as shown in the settings window.
pub const SOURCE_TITLE: &str = "牛津高阶英汉双解词典（第 10 版）";

pub struct Args {
    pub mdx: PathBuf,
    pub mdds: Vec<PathBuf>,
    pub ecdict: PathBuf,
    /// Output folder.
    pub out: PathBuf,
    pub previous: Option<PathBuf>,
}

#[derive(Debug, Default, serde::Serialize)]
pub struct Report {
    pub records: u64,
    pub pages_skipped: u64,
    pub counts: write::Counts,
    pub parse: parse::ParseStats,
    pub audio: audio::AudioCounts,
    pub renames: Option<previous::RenameCounts>,
    pub seconds: f64,
}

static INTERRUPTED: AtomicBool = AtomicBool::new(false);

/// Called from the Ctrl+C handler: the conversion stops at its next check and removes its
/// temporary files.
pub fn interrupt() {
    INTERRUPTED.store(true, Ordering::SeqCst);
}

fn check_interrupted() -> Result<()> {
    if INTERRUPTED.load(Ordering::SeqCst) {
        bail!("interrupted");
    }
    Ok(())
}

pub fn sha256_file(path: &Path) -> Result<String> {
    let mut hasher = Sha256::new();
    let mut file = File::open(path).with_context(|| format!("reading {}", path.display()))?;
    let mut buf = vec![0; 1 << 16];
    loop {
        let n = file.read(&mut buf)?;
        if n == 0 {
            break;
        }
        hasher.update(&buf[..n]);
    }
    Ok(hasher
        .finalize()
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect())
}

fn file_name(path: &Path) -> String {
    path.file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default()
}

/// Runs the whole conversion. `built_at` goes to `meta.built_at` (the only value that
/// differs between two runs on the same input).
pub fn run(args: &Args, built_at: &str) -> Result<Report> {
    let started = Instant::now();
    std::fs::create_dir_all(&args.out)
        .with_context(|| format!("creating {}", args.out.display()))?;
    let text_target = args.out.join(OXFORD_FILE_NAME);
    let audio_target = args.out.join(OXFORD_AUDIO_FILE_NAME);
    for target in [&audio_target, &text_target] {
        output::ensure_replaceable(target)?;
        output::remove_stale(target)?;
    }
    let mut temps = TempFiles::default();

    let ecdict = ecdict::Ecdict::open(&args.ecdict)?;
    let source = source::read(&args.mdx, &check_interrupted)?;
    check_interrupted()?;
    let prepared = write::prepare(&source.entries, &ecdict);
    let previous_items = match &args.previous {
        Some(p) => Some(previous::read_previous(p)?),
        None => None,
    };

    let mut report = Report {
        records: source.records,
        pages_skipped: source.pages,
        parse: source.stats.clone(),
        ..Report::default()
    };

    // Audio first, then the text dictionary (3).
    let wanted: BTreeSet<String> = source
        .entries
        .iter()
        .flat_map(|e| e.entry.sounds.iter().cloned())
        .collect();
    let work = temps.next(&audio_target);
    let mut conn = Connection::open(&work)?;
    report.audio = audio::write(&mut conn, &args.mdds, &wanted, &check_interrupted)?;
    write_meta(
        &conn,
        &[
            ("format", AUDIO_FORMAT.to_owned()),
            ("format_version", AUDIO_FORMAT_VERSION.to_string()),
            ("source", OXFORD_SOURCE.to_owned()),
            ("source_title", SOURCE_TITLE.to_owned()),
            ("converter_version", CONVERTER_VERSION.to_string()),
            ("file_count", report.audio.copied.to_string()),
            ("built_at", built_at.to_owned()),
        ],
    )?;
    check_interrupted()?;
    output::finish(conn, &mut temps, &audio_target)?;

    let work = temps.next(&text_target);
    let mut conn = Connection::open(&work)?;
    report.counts = write::write(&mut conn, &prepared, &ecdict)?;
    check_interrupted()?;
    if let Some(old) = &previous_items {
        let new = previous::read_items(&conn)?;
        let (renames, counts) = previous::renames(old, &new);
        previous::write(&conn, &renames)?;
        report.renames = Some(counts);
    }
    let mdx_size = std::fs::metadata(&args.mdx)?.len();
    write_meta(
        &conn,
        &[
            ("format", FORMAT.to_owned()),
            ("format_version", FORMAT_VERSION.to_string()),
            ("source", OXFORD_SOURCE.to_owned()),
            ("source_title", SOURCE_TITLE.to_owned()),
            ("id_scheme", "b".to_owned()),
            ("source_file", file_name(&args.mdx)),
            ("source_size", mdx_size.to_string()),
            ("source_sha256", sha256_file(&args.mdx)?),
            ("ecdict_source_commit", ecdict.source_commit.clone()),
            ("ecdict_selection", ecdict.selection.clone()),
            ("ecdict_format_version", ecdict.format_version.clone()),
            ("ecdict_sha256", ecdict.sha256.clone()),
            ("converter_version", CONVERTER_VERSION.to_string()),
            ("entry_count", report.counts.entries.to_string()),
            ("built_at", built_at.to_owned()),
        ],
    )?;
    check_interrupted()?;
    output::finish(conn, &mut temps, &text_target)?;

    report.seconds = started.elapsed().as_secs_f64();
    Ok(report)
}

fn write_meta(conn: &Connection, rows: &[(&str, String)]) -> Result<()> {
    for (key, value) in rows {
        conn.execute(
            "INSERT INTO meta (key, value) VALUES (?1, ?2)",
            params![key, value],
        )?;
    }
    Ok(())
}

/// Current UTC time as `YYYY-MM-DDTHH:MM:SSZ`.
pub fn now_utc() -> String {
    let secs = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| d.as_secs()) as i64;
    let days = secs.div_euclid(86_400);
    let rem = secs.rem_euclid(86_400);
    // Civil date from days since 1970-01-01 (Howard Hinnant's algorithm).
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = yoe + era * 400 + i64::from(m <= 2);
    format!(
        "{y:04}-{m:02}-{d:02}T{:02}:{:02}:{:02}Z",
        rem / 3600,
        rem % 3600 / 60,
        rem % 60
    )
}
