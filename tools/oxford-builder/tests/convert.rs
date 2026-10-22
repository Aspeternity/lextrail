//! End-to-end tests of the converter on made-up entries (`tests/fixtures/`), built into a
//! small MDX / MDD at test time (`docs/specs/m1.5-oxford.md` section 11).

use std::fs;
use std::path::{Path, PathBuf};

use lextrail_dict::ext::schema::FORMAT;
use lextrail_oxford_builder::{Args, run};
use rusqlite::Connection;

fn fixture(name: &str) -> String {
    fs::read_to_string(
        Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("tests/fixtures")
            .join(name),
    )
    .unwrap()
}

/// Lookup keys of each fixture, as the MDX would list them.
const RECORDS: &[(&str, &str)] = &[
    ("blorf", "blorf-verb.html"),
    ("blorf", "blorf-noun.html"),
    ("blorft", "blorf-verb.html"),
    ("blorft", "blorf-noun.html"),
    ("blorfs", "blorf-verb.html"),
    ("blorfs", "blorf-noun.html"),
    ("blorf out", "blorf-verb.html"),
    ("blorfer", "blorf-verb.html"),
    ("bloarf", "blorf-noun.html"),
    ("snib", "snib-1.html"),
    ("snib", "snib-2.html"),
    ("Help page", "page.html"),
];

struct Source {
    dir: tempfile::TempDir,
}

impl Source {
    /// MDX from `records` (key, HTML), an MDD with three of the sounds, and a tiny ECDICT.
    fn new(records: &[(&str, String)]) -> Self {
        let dir = tempfile::tempdir().unwrap();
        let mut mdx = mdictlib::MdxBuilder::new();
        for (key, html) in records {
            mdx.add_entry(key, html).unwrap();
        }
        mdx.finish(fs::File::create(dir.path().join("t.mdx")).unwrap())
            .unwrap();
        let mut mdd = mdictlib::MddBuilder::new();
        for name in ["blorf__gb_1.ogg", "blorf__us_1.ogg", "blorf__gbs_1.ogg"] {
            mdd.add_resource(&format!("\\audio\\{name}"), name.as_bytes())
                .unwrap();
        }
        mdd.finish(fs::File::create(dir.path().join("t.mdd")).unwrap())
            .unwrap();
        let ecdict = Connection::open(dir.path().join("ecdict.db")).unwrap();
        ecdict.execute_batch(lextrail_dict::schema::SCHEMA).unwrap();
        ecdict
            .execute_batch(
                "INSERT INTO meta VALUES ('source_commit', 'test'), ('selection', 'test'),
                 ('format_version', '2');
                 INSERT INTO forms VALUES ('blorft', 'blorf', 'p'), ('blorft', 'blorf', 'd');",
            )
            .unwrap();
        Source { dir }
    }

    fn fixtures() -> Self {
        Self::new(&records(|h| h))
    }

    fn args(&self, out: &Path) -> Args {
        Args {
            mdx: self.dir.path().join("t.mdx"),
            mdds: vec![self.dir.path().join("t.mdd")],
            ecdict: self.dir.path().join("ecdict.db"),
            out: out.to_owned(),
            previous: None,
        }
    }
}

fn records(edit: impl Fn(String) -> String) -> Vec<(&'static str, String)> {
    RECORDS
        .iter()
        .map(|(k, f)| (*k, edit(fixture(f))))
        .collect()
}

fn rows(db: &Path, sql: &str) -> Vec<String> {
    let conn = Connection::open(db).unwrap();
    let mut stmt = conn.prepare(sql).unwrap();
    let n = stmt.column_count();
    stmt.query_map([], |r| {
        Ok((0..n)
            .map(|i| match r.get_ref(i).unwrap() {
                rusqlite::types::ValueRef::Null => "-".to_owned(),
                rusqlite::types::ValueRef::Integer(v) => v.to_string(),
                rusqlite::types::ValueRef::Real(v) => v.to_string(),
                rusqlite::types::ValueRef::Text(t) => String::from_utf8_lossy(t).into_owned(),
                rusqlite::types::ValueRef::Blob(b) => format!("<{} bytes>", b.len()),
            })
            .collect::<Vec<_>>()
            .join(" | "))
    })
    .unwrap()
    .map(Result::unwrap)
    .collect()
}

fn file_names(dir: &Path) -> Vec<String> {
    let mut names: Vec<String> = fs::read_dir(dir)
        .unwrap()
        .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
        .collect();
    names.sort();
    names
}

#[test]
fn converts_the_fixtures() {
    let src = Source::fixtures();
    let out = src.dir.path().join("out");
    let report = run(&src.args(&out), "2026-01-01T00:00:00Z").unwrap();
    let db = out.join("oalecd10.db");

    assert_eq!(file_names(&out), ["oalecd10-audio.db", "oalecd10.db"]);
    assert_eq!(report.records, 12);
    assert_eq!(report.pages_skipped, 1);
    assert_eq!(report.counts.entries, 4);
    assert_eq!(report.parse.unknown_classes.get("t-zzz-new"), Some(&1));
    assert_eq!(report.counts.senses_alt_fingerprint, 2);
    assert_eq!(report.counts.duplicate_entry_keys, 1);

    let meta = rows(&db, "SELECT key, value FROM meta ORDER BY key");
    assert!(meta.contains(&format!("format | {FORMAT}")));
    assert!(meta.contains(&"format_version | 1".to_owned()));
    assert!(meta.contains(&"source | oalecd10".to_owned()));
    assert!(meta.contains(&"id_scheme | b".to_owned()));
    assert!(meta.contains(&"ecdict_source_commit | test".to_owned()));
    assert!(meta.contains(&"built_at | 2026-01-01T00:00:00Z".to_owned()));
    assert!(meta.contains(&"entry_count | 4".to_owned()));

    assert_eq!(
        rows(
            &db,
            "SELECT entry_key, headword, pos, phon_gb, phon_us, sound_gb, sound_us, cefr, oxford,
             opal, ord FROM entries ORDER BY id"
        ),
        [
            "blorf||verb | blorf | verb | blɔːf | blɔːrf | audio/blorf__gb_1.ogg | \
             audio/blorf__us_1.ogg | A2 | 3000 | W | 0",
            "blorf||noun | blorf | noun | blɔːf | - | - | - | - | - | - | 1",
            "snib||noun | snib | noun | - | - | - | - | - | - | - | 0",
            "snib||noun|2 | snib | noun | - | - | - | - | - | - | - | 1",
        ]
    );

    // Groups in source order; the idiom nested in sense 2 is the first idiom.
    assert_eq!(
        rows(
            &db,
            "SELECT grp, label, pos FROM groups WHERE entry_id = 1 ORDER BY ord"
        ),
        [
            "s | - | verb",
            "i1 | blorf the ˈsnib | -",
            "i2 | ˌblorf and ˈsnib | -",
            "p1 | ˌblorf sb↔ˈout | -",
            "d1 | ˈblorfer | noun",
        ]
    );
    // Senses numbered within their groups; the derivative has none.
    assert_eq!(
        rows(
            &db,
            "SELECT grp, sense FROM senses WHERE entry_id = 1 ORDER BY id"
        ),
        ["s | 1", "s | 2", "i1 | 1", "i2 | 1", "p1 | 1", "p1 | 2"]
    );
    assert_eq!(
        rows(
            &db,
            "SELECT cefr, gram, labels, heading_en, heading_zh, patterns, def_en, def_zh, xrefs
             FROM senses WHERE entry_id = 1 AND grp = 's' ORDER BY sense"
        ),
        [
            "A2 | [intransitive] | informal | making noise | 发声 | [\"~ at sb\"] | \
             to make a low snibbing noise | 发出低沉的噼噼声 | \
             [{\"kind\":\"syn\",\"label\":\"SYN\",\"targets\":[\"snib\"]}]",
            "- | - | - | making noise | 发声 | - | to blorf something away | 噼走 | -",
        ]
    );
    assert_eq!(
        rows(
            &db,
            "SELECT sense, ord, pattern, en, zh, sound_gb FROM examples WHERE entry_id = 1
             ORDER BY id"
        ),
        [
            "1 | 1 | ~ loudly | The snib blorfed loudly. | 噼噼大响。 | audio/blorf__gbs_1.ogg",
            "1 | 2 | - | Stop blorfing! | 别噼了！ | -",
        ]
    );
    // Fingerprints: the definition, or `alt:` and the English text (element texts joined as
    // written, Chinese left out).
    let fingerprints = rows(
        &db,
        "SELECT fingerprint FROM senses WHERE entry_id = 2 ORDER BY sense",
    );
    assert_eq!(
        fingerprints,
        [
            lextrail_dict::ext::text::text_hash("a low snibbing noise"),
            format!(
                "alt:{}",
                lextrail_dict::ext::text::text_hash("seesniba blorf of snibs")
            ),
        ]
    );

    assert_eq!(
        rows(
            &db,
            "SELECT ord, kind, text, sound_gb FROM forms WHERE entry_id = 1 ORDER BY ord"
        ),
        [
            "1 | root | blorf | -",
            "2 | past | blorft | audio/blorft__gb_1.ogg",
            "3 | pastpart | blorft | -",
        ]
    );
    assert_eq!(
        rows(&db, "SELECT entry_id, label, text FROM variants"),
        ["2 | US | bloarf"]
    );
    assert_eq!(
        rows(&db, "SELECT entry_id, grp, kind, text FROM notes"),
        ["1 | - | etym | ORIGIN Made up for tests."]
    );

    assert_eq!(
        rows(
            &db,
            "SELECT key, entry_id, relation, kinds, via, grp, ord FROM keys
             ORDER BY lower(key), ord"
        ),
        [
            "bloarf | 2 | variant | - | bloarf | - | 0",
            "blorf | 1 | headword | - | - | - | 0",
            "blorf | 2 | headword | - | - | - | 1",
            "blorf out | 1 | phrasal | - | ˌblorf sb↔ˈout | p1 | 0",
            "blorfer | 1 | derivative | - | ˈblorfer | d1 | 0",
            "blorfs | 1 | inflection | 3 | blorf | - | 0",
            "blorfs | 2 | inflection | s | blorf | - | 1",
            "blorft | 1 | inflection | p d | blorf | - | 0",
            "blorft | 2 | other | - | - | - | 1",
            "snib | 3 | headword | - | - | - | 0",
            "snib | 4 | headword | - | - | - | 1",
        ]
    );

    // Headword and verb form sounds, not the example's; one is not in the MDD.
    assert_eq!(report.audio.referenced, 3);
    assert_eq!(report.audio.copied, 2);
    assert_eq!(report.audio.missing, 1);
    assert_eq!(
        rows(
            &out.join("oalecd10-audio.db"),
            "SELECT name, data FROM audio ORDER BY name"
        ),
        [
            "audio/blorf__gb_1.ogg | <15 bytes>",
            "audio/blorf__us_1.ogg | <15 bytes>"
        ]
    );
}

fn content(db: &Path) -> Vec<String> {
    let tables = rows(
        db,
        "SELECT name FROM sqlite_master WHERE type = 'table' ORDER BY name",
    );
    let mut out = Vec::new();
    for t in tables {
        out.push(format!("== {t}"));
        out.extend(
            rows(db, &format!("SELECT * FROM {t}"))
                .into_iter()
                .filter(|r| !r.starts_with("built_at")),
        );
    }
    out
}

#[test]
fn output_is_reproducible() {
    let src = Source::fixtures();
    let (a, b) = (src.dir.path().join("a"), src.dir.path().join("b"));
    run(&src.args(&a), "2026-01-01T00:00:00Z").unwrap();
    run(&src.args(&b), "2026-02-02T00:00:00Z").unwrap();
    for file in ["oalecd10.db", "oalecd10-audio.db"] {
        assert_eq!(content(&a.join(file)), content(&b.join(file)), "{file}");
    }
}

#[test]
fn a_second_conversion_replaces_the_files() {
    let src = Source::fixtures();
    let out = src.dir.path().join("out");
    run(&src.args(&out), "2026-01-01T00:00:00Z").unwrap();
    // Leftovers of a killed run are cleaned up too.
    fs::write(out.join("oalecd10.db.tmp-dead"), b"").unwrap();
    run(&src.args(&out), "2026-02-02T00:00:00Z").unwrap();
    assert_eq!(file_names(&out), ["oalecd10-audio.db", "oalecd10.db"]);
    assert!(
        rows(
            &out.join("oalecd10.db"),
            "SELECT value FROM meta WHERE key = 'built_at'"
        ) == ["2026-02-02T00:00:00Z"]
    );
}

#[test]
fn ids_of_a_new_export_are_mapped_to_the_old_ones() {
    let src = Source::fixtures();
    let v1 = src.dir.path().join("v1");
    run(&src.args(&v1), "2026-01-01T00:00:00Z").unwrap();

    // The new export: sense 1 of the verb removed (sense 2 becomes 1), the noun's first
    // definition reworded, and a new entry.
    let mut changed = records(|h| {
        let start = h.find(r#"<span class="t-sn-blk" shcut="y"><ol start="1">"#);
        let h = match start {
            Some(s) => {
                let end = h[s..].find("</ol></span>").unwrap() + s + "</ol></span>".len();
                format!("{}{}", &h[..s], &h[end..])
            }
            None => h,
        };
        h.replace("a low snibbing noise", "a quiet snibbing noise")
    });
    changed.push((
        "aardblorf",
        r#"<div class="entry t-entry t-oalecd10" data-hkoup-entry="5"><span class="t-hposbk"><span class="t-h">aardblorf</span></span><span class="t-sn-gs"><span class="t-sn-g"><div class="t-def"><span class="t-eng">a new word</span></div></span></span></div>"#
            .to_owned(),
    ));
    let src2 = Source::new(&changed);
    let mut args = src2.args(&src2.dir.path().join("v2"));
    args.previous = Some(v1.join("oalecd10.db"));
    let report = run(&args, "2026-02-02T00:00:00Z").unwrap();
    let renames = report.renames.unwrap();
    assert_eq!(
        rows(
            &args.out.join("oalecd10.db"),
            "SELECT old_id, new_id, how FROM sense_renames ORDER BY old_id"
        ),
        [
            "oalecd10/blorf||noun/s/1 | - | removed",
            "oalecd10/blorf||verb/s/1 | - | removed",
            "oalecd10/blorf||verb/s/2 | oalecd10/blorf||verb/s/1 | same_entry",
        ]
    );
    assert_eq!(renames.removed, 2);
    assert_eq!(renames.same_entry, 1);
    // Everything else (the idioms, phrasal verb and derivative, both snibs) is unchanged:
    // 10 senses and 4 groups, minus the 3 above.
    assert_eq!(renames.unchanged, 11);
}

#[test]
fn differing_records_of_one_entry_stop_the_conversion() {
    let mut recs = records(|h| h);
    recs.push(("snibs", fixture("snib-1.html").replace("small", "tiny")));
    let src = Source::new(&recs);
    let out = src.dir.path().join("out");
    let err = run(&src.args(&out), "x").unwrap_err().to_string();
    assert!(err.contains("entry 3"), "{err}");
    assert!(file_names(&out).is_empty());
}

#[cfg(windows)]
#[test]
fn a_file_in_use_is_not_replaced() {
    let src = Source::fixtures();
    let out: PathBuf = src.dir.path().join("out");
    fs::create_dir_all(&out).unwrap();
    fs::write(out.join("oalecd10.db"), b"in use").unwrap();
    let open = fs::File::open(out.join("oalecd10.db")).unwrap();
    let err = run(&src.args(&out), "x").unwrap_err().to_string();
    assert!(err.contains("in use"), "{err}");
    drop(open);
    assert_eq!(fs::read(out.join("oalecd10.db")).unwrap(), b"in use");
    assert_eq!(file_names(&out), ["oalecd10.db"]);
}
