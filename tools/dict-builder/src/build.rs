//! Writes the compact dictionary file (`docs/specs/m1-dict.md` section 5).
//!
//! The output is deterministic: the same input and builder version give a byte-identical
//! file (rows in CSV order, no timestamps), so CI caches and comparisons work.

use std::collections::{BTreeSet, HashSet};
use std::fs;
use std::io::Read;
use std::path::Path;

use anyhow::{Context, Result};
use lextrail_dict::schema::{INDEXES, SCHEMA};
use lextrail_dict::{FORMAT_VERSION, normalize_phonetic, parse_exchange, split_translation};
use rusqlite::{Connection, params};

use crate::row::{Columns, Row};

/// Name of the selection rule, stored in `meta.selection`.
pub const SELECTION: &str = "b-plus";

/// Provenance written to `meta`.
pub struct Provenance<'a> {
    pub url: &'a str,
    pub commit: &'a str,
    pub sha256: &'a str,
}

#[derive(Debug, Default, PartialEq, Eq)]
pub struct Stats {
    pub rows: usize,
    pub entries: usize,
    pub senses: usize,
    pub forms: usize,
    /// Selected rows whose translation split into no senses (dropped).
    pub empty: usize,
    /// Headwords of the whole source that are not entries (`extra_words`).
    pub extra_words: usize,
}

/// Reads ECDICT CSV from `csv` and writes the dictionary to `out`, replacing it. The file
/// is written next to `out` first and renamed when complete.
pub fn build(csv: impl Read, out: &Path, provenance: &Provenance) -> Result<Stats> {
    let tmp = out.with_extension("db.tmp");
    let _ = fs::remove_file(&tmp);
    let stats = write(csv, &tmp, provenance);
    if stats.is_err() {
        let _ = fs::remove_file(&tmp);
    }
    let stats = stats?;
    if out.exists() {
        fs::remove_file(out).with_context(|| format!("replacing {}", out.display()))?;
    }
    fs::rename(&tmp, out)?;
    Ok(stats)
}

fn write(csv: impl Read, path: &Path, provenance: &Provenance) -> Result<Stats> {
    let mut reader = csv::ReaderBuilder::new().from_reader(csv);
    let columns = Columns::from_header(reader.headers()?)?;
    let rows = reader
        .records()
        .enumerate()
        .map(|(i, record)| {
            // Line numbers count the header as line 1.
            let line = i + 2;
            columns
                .row(&record.with_context(|| format!("CSV line {line}"))?)
                .with_context(|| format!("CSV line {line}"))
        })
        .collect::<Result<Vec<Row>>>()?;

    let mut conn = Connection::open(path)?;
    conn.execute_batch(
        "PRAGMA page_size = 4096; PRAGMA journal_mode = OFF; PRAGMA synchronous = OFF;",
    )?;
    conn.execute_batch(SCHEMA)?;

    let mut stats = Stats {
        rows: rows.len(),
        ..Stats::default()
    };
    let tx = conn.transaction()?;
    let meta = [
        ("format_version", FORMAT_VERSION.to_string()),
        ("source", "ECDICT".to_owned()),
        ("source_url", provenance.url.to_owned()),
        ("source_commit", provenance.commit.to_owned()),
        ("source_sha256", provenance.sha256.to_owned()),
        ("selection", SELECTION.to_owned()),
    ];
    for (key, value) in meta {
        tx.execute(
            "INSERT INTO meta (key, value) VALUES (?1, ?2)",
            params![key, value],
        )?;
    }

    let mut included = HashSet::new();
    {
        let mut entry = tx.prepare(
            "INSERT INTO entries (id, word, phonetic, collins, oxford, bnc, frq, tags)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
        )?;
        let mut sense = tx.prepare(
            "INSERT INTO senses (entry_id, ord, pos, domain, text) VALUES (?1, ?2, ?3, ?4, ?5)",
        )?;
        for row in rows.iter().filter(|r| r.in_core()) {
            let senses = split_translation(&row.translation);
            if senses.is_empty() {
                stats.empty += 1;
                continue;
            }
            stats.entries += 1;
            let id = stats.entries as i64;
            let word = row.word.trim();
            let phonetic = Some(normalize_phonetic(&row.phonetic)).filter(|p| !p.is_empty());
            let tags = row.tag.split_whitespace().collect::<Vec<_>>().join(" ");
            entry.execute(params![
                id,
                word,
                phonetic,
                nonzero(row.collins),
                row.oxford.then_some(1),
                nonzero(row.bnc),
                nonzero(row.frq),
                Some(tags).filter(|t| !t.is_empty()),
            ])?;
            for (ord, s) in senses.iter().enumerate() {
                sense.execute(params![id, ord as i64, s.pos, s.domain, s.text])?;
            }
            stats.senses += senses.len();
            included.insert(word.to_owned());
        }
    }

    let forms = collect_forms(&rows, &included);
    {
        let mut insert = tx.prepare("INSERT INTO forms (form, lemma, kind) VALUES (?1, ?2, ?3)")?;
        for (form, lemma, kind) in &forms {
            insert.execute(params![form, lemma, kind])?;
        }
    }
    stats.forms = forms.len();

    let extra = collect_extra_words(&rows, &included);
    {
        let mut insert = tx.prepare("INSERT INTO extra_words (word) VALUES (?1)")?;
        for word in &extra {
            insert.execute([word])?;
        }
    }
    stats.extra_words = extra.len();
    tx.commit()?;

    conn.execute_batch(INDEXES)?;
    conn.execute_batch("VACUUM;")?;
    conn.close().map_err(|(_, e)| e)?;
    Ok(stats)
}

/// `form -> lemma` pairs from every row's `exchange` (not only selected rows), kept when the
/// lemma is in the dictionary. Sorted, without duplicates.
fn collect_forms(rows: &[Row], included: &HashSet<String>) -> BTreeSet<(String, String, String)> {
    let mut forms = BTreeSet::new();
    let mut add = |form: &str, lemma: &str, kind: &str| {
        let (form, lemma) = (form.trim(), lemma.trim());
        if form != lemma && !form.is_empty() && included.contains(lemma) {
            forms.insert((form.to_owned(), lemma.to_owned(), kind.to_owned()));
        }
    };
    for row in rows {
        let exchange = parse_exchange(&row.exchange);
        for (kind, form) in &exchange.forms {
            add(form, &row.word, kind.code());
        }
        if let Some(lemma) = &exchange.lemma {
            for kind in &exchange.lemma_kinds {
                add(&row.word, lemma, kind.code());
            }
        }
    }
    forms
}

/// Words of every row (not only selected ones) that are plain ASCII letters, at least two
/// of them, and not an entry in any case; lowercase, sorted. Only used to decide whether
/// to join a word broken across lines (D044 supplement, `m1-dict.md` 10.2).
fn collect_extra_words(rows: &[Row], included: &HashSet<String>) -> BTreeSet<String> {
    let included: HashSet<String> = included.iter().map(|w| w.to_lowercase()).collect();
    rows.iter()
        .map(|r| r.word.trim().to_ascii_lowercase())
        .filter(|w| w.len() >= 2 && w.bytes().all(|b| b.is_ascii_lowercase()))
        .filter(|w| !included.contains(w))
        .collect()
}

fn nonzero(n: u32) -> Option<u32> {
    (n > 0).then_some(n)
}

#[cfg(test)]
mod tests {
    use super::*;

    const CSV: &str = "\
word,phonetic,definition,translation,pos,collins,oxford,tag,bnc,frq,exchange,detail,audio
launch,lɔ:ntʃ,,\"n. 下水, 汽艇, 发射\\nvt. 使下水, 发射\",,4,1,cet4 cet6,2236,1789,p:launched/d:launched/i:launching/3:launches/s:launches,,
launched,lɔ:ntʃt,,v. 发射( launch的过去式和过去分词 ),,,,,0,0,0:launch/1:pd,,
real,'ri\u{04D9}l,,\"a. 真的, 真实的, 实数的\\nn. 实数, 现实\",,,,zk  gk,425,305,,,
give up,,,\"放弃, 停止\",,,,,0,0,,,
butylbenzene,,,[化] 丁苯,,,,,0,0,,,
butylcarboxylic acid,,,\"[医] 戊酸, 缬草酸\",,,,,0,0,,,
gamy,ɡeimi,,[医] ,,,,,0,0,s:gamies,,
";

    const PROVENANCE: Provenance = Provenance {
        url: "https://example.invalid/ecdict.csv",
        commit: "abc1234",
        sha256: "00",
    };

    fn build_fixture(dir: &Path) -> (Stats, Connection) {
        let out = dir.join("dict.db");
        let stats = build(CSV.as_bytes(), &out, &PROVENANCE).unwrap();
        (stats, Connection::open(&out).unwrap())
    }

    #[test]
    fn selects_and_splits() {
        let dir = tempfile::tempdir().unwrap();
        let (stats, conn) = build_fixture(dir.path());
        assert_eq!(
            stats,
            Stats {
                rows: 7,
                entries: 4,
                senses: 5 + 1 + 5 + 2,
                forms: 4 + 1,
                empty: 1,
                extra_words: 2,
            }
        );
        let words: Vec<String> = conn
            .prepare("SELECT word FROM entries ORDER BY id")
            .unwrap()
            .query_map([], |r| r.get(0))
            .unwrap()
            .collect::<Result<_, _>>()
            .unwrap();
        assert_eq!(words, ["launch", "launched", "real", "give up"]);
    }

    #[test]
    fn stores_entry_fields() {
        let dir = tempfile::tempdir().unwrap();
        let (_, conn) = build_fixture(dir.path());
        // phonetic, collins, oxford, bnc, tags
        type Fields = (
            Option<String>,
            Option<i64>,
            Option<i64>,
            Option<i64>,
            Option<String>,
        );
        let launch: Fields = conn
            .query_row(
                "SELECT phonetic, collins, oxford, bnc, tags FROM entries WHERE word = 'launch'",
                [],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?)),
            )
            .unwrap();
        assert_eq!(
            launch,
            (
                Some("lɔ:ntʃ".into()),
                Some(4),
                Some(1),
                Some(2236),
                Some("cet4 cet6".into())
            )
        );
        let real: (String, Option<i64>, Option<String>) = conn
            .query_row(
                "SELECT phonetic, oxford, tags FROM entries WHERE word = 'real'",
                [],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
            )
            .unwrap();
        assert_eq!(real, ("'ri\u{0259}l".into(), None, Some("zk gk".into())));
        let give_up: Option<String> = conn
            .query_row(
                "SELECT phonetic FROM entries WHERE word = 'give up'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(give_up, None);
    }

    #[test]
    fn stores_senses_in_order() {
        let dir = tempfile::tempdir().unwrap();
        let (_, conn) = build_fixture(dir.path());
        let senses: Vec<(Option<String>, String)> = conn
            .prepare(
                "SELECT pos, text FROM senses JOIN entries ON entries.id = entry_id
                 WHERE word = 'real' ORDER BY ord",
            )
            .unwrap()
            .query_map([], |r| Ok((r.get(0)?, r.get(1)?)))
            .unwrap()
            .collect::<Result<_, _>>()
            .unwrap();
        let a = Some("a.".to_owned());
        let n = Some("n.".to_owned());
        assert_eq!(
            senses,
            [
                (a.clone(), "真的".into()),
                (a.clone(), "真实的".into()),
                (a, "实数的".into()),
                (n.clone(), "实数".into()),
                (n, "现实".into()),
            ]
        );
    }

    #[test]
    fn forms_point_to_included_lemmas() {
        let dir = tempfile::tempdir().unwrap();
        let (_, conn) = build_fixture(dir.path());
        let forms: Vec<(String, String, String)> = conn
            .prepare("SELECT form, lemma, kind FROM forms ORDER BY form, kind")
            .unwrap()
            .query_map([], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)))
            .unwrap()
            .collect::<Result<_, _>>()
            .unwrap();
        let f = |form: &str, kind: &str| (form.to_owned(), "launch".to_owned(), kind.to_owned());
        // `launches` is both 3rd person and plural; `launched` comes from both rows.
        // `gamies` is dropped because `gamy` has no senses.
        assert_eq!(
            forms,
            [
                f("launched", "d"),
                f("launched", "p"),
                f("launches", "3"),
                f("launches", "s"),
                f("launching", "i")
            ]
        );
        let hit: String = conn
            .query_row(
                "SELECT lemma FROM forms WHERE form = 'LAUNCHING' COLLATE NOCASE",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(hit, "launch");
    }

    #[test]
    fn extra_words_are_the_other_plain_words() {
        let dir = tempfile::tempdir().unwrap();
        let (_, conn) = build_fixture(dir.path());
        let words: Vec<String> = conn
            .prepare("SELECT word FROM extra_words ORDER BY word")
            .unwrap()
            .query_map([], |r| r.get(0))
            .unwrap()
            .collect::<Result<_, _>>()
            .unwrap();
        // Not entries: `butylbenzene` (not selected) and `gamy` (no senses). Phrases and
        // entries are not listed.
        assert_eq!(words, ["butylbenzene", "gamy"]);
    }

    #[test]
    fn writes_meta() {
        let dir = tempfile::tempdir().unwrap();
        let (_, conn) = build_fixture(dir.path());
        let get = |key: &str| -> String {
            conn.query_row("SELECT value FROM meta WHERE key = ?1", [key], |r| r.get(0))
                .unwrap()
        };
        assert_eq!(get("format_version"), FORMAT_VERSION.to_string());
        assert_eq!(get("source_commit"), "abc1234");
        assert_eq!(get("selection"), SELECTION);
    }

    #[test]
    fn output_is_deterministic_and_replaced() {
        let dir = tempfile::tempdir().unwrap();
        let a = dir.path().join("a.db");
        let b = dir.path().join("b.db");
        build(CSV.as_bytes(), &a, &PROVENANCE).unwrap();
        build(CSV.as_bytes(), &b, &PROVENANCE).unwrap();
        assert_eq!(fs::read(&a).unwrap(), fs::read(&b).unwrap());
        // Rebuilding over an existing file replaces it.
        build(CSV.as_bytes(), &a, &PROVENANCE).unwrap();
        assert_eq!(fs::read(&a).unwrap(), fs::read(&b).unwrap());
        assert!(!a.with_extension("db.tmp").exists());
    }

    #[test]
    fn bad_input_leaves_no_file() {
        let dir = tempfile::tempdir().unwrap();
        let out = dir.path().join("dict.db");
        let csv = "word,phonetic,definition,translation,pos,collins,oxford,tag,bnc,frq,exchange\nx,,,n. 叉,,,,,abc,,\n";
        let err = build(csv.as_bytes(), &out, &PROVENANCE).unwrap_err();
        assert!(format!("{err:#}").contains("CSV line 2"), "{err:#}");
        assert!(!out.exists());
        assert!(!out.with_extension("db.tmp").exists());
    }
}
