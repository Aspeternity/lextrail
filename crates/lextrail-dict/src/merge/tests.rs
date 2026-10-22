//! Merged lookups on a small made-up Oxford dictionary and the ECDICT test dictionary
//! (`docs/specs/m1.5-oxford.md` 11).

use std::path::Path;

use rusqlite::{Connection, params};

use super::*;
use crate::dictionary::tests::fixture;
use crate::ext::schema::{FORMAT, FORMAT_VERSION, INDEXES, SCHEMA};

const PAST_OF_BLORF: &str = r#"[{"kind":"ptof","label":"past tense of","targets":["blorf1"]}]"#;
const SAME_AS_KOLOR: &str = r#"[{"kind":"eq","label":"=","targets":["kolor noun"]}]"#;
const SAME_AS_GIVE_UP: &str = r#"[{"kind":"eq","label":"=","targets":["give up"]}]"#;

/// id, headword, pos, CEFR, Oxford 3000 / 5000, British phonetic.
type EntryRow = (
    i64,
    &'static str,
    &'static str,
    Option<&'static str>,
    Option<u32>,
    Option<&'static str>,
);

const ENTRIES: &[EntryRow] = &[
    (1, "blorf", "verb", Some("B1"), None, Some("blɔːf")),
    (2, "blorf", "noun", None, None, Some("blɔːf")),
    (3, "blorft", "", None, None, Some("blɔːft")),
    (4, "snib", "noun", None, Some(3000), Some("snɪb")),
    (5, "snab", "verb", None, None, Some("snæb")),
    (6, "glomp", "noun", None, None, Some("ɡlɒmp")),
    (7, "glimp", "verb", None, None, Some("ɡlɪmp")),
    (8, "Zorb", "noun", None, None, Some("zɔːb")),
    (9, "zorb", "verb", None, None, Some("zɔːb")),
    (10, "kolor", "noun", None, None, Some("ˈkʌlə")),
    (11, "launch", "verb", None, None, None),
    (12, "improve", "verb", Some("A2"), None, Some("ɪmˈpruːv")),
    (13, "good", "adjective", Some("A1"), None, Some("ɡʊd")),
    (14, "give", "verb", Some("A1"), None, Some("ɡɪv")),
    (15, "snib blorf", "noun", None, None, None),
    (16, "wug", "noun", None, None, Some("wʌɡ")),
    (17, "gaolf", "noun", None, None, Some("ɡeɪlf")),
    (18, "blorf-out", "noun", None, None, None),
    (19, "areb", "noun", None, None, None),
    (20, "KLOMPS", "abbreviation", None, None, None),
    (21, "klomp", "noun", None, None, None),
    (22, "-snorb", "suffix", None, None, None),
    (23, "glimp-", "prefix", None, None, None),
];

/// entry, group, label.
const GROUPS: &[(i64, &str, Option<&str>)] = &[
    (1, "i1", Some("blorf the snib")),
    (1, "p1", Some("blorf out")),
    (1, "d1", Some("blorfer")),
    (14, "p1", Some("give sth↔up")),
    (14, "p2", Some("give large")),
];

/// entry, group, sense, English and Chinese definition, cross-references.
type SenseRow = (
    i64,
    &'static str,
    u32,
    Option<&'static str>,
    Option<&'static str>,
    Option<&'static str>,
);

const SENSES: &[SenseRow] = &[
    (1, "s", 1, Some("to make a low sound"), Some("低鸣"), None),
    (1, "s", 2, Some("to say quickly"), Some("快说"), None),
    (1, "i1", 1, Some("to fail"), Some("失败"), None),
    (1, "p1", 1, Some("to shout"), Some("喊出"), None),
    (2, "s", 1, Some("a low sound"), Some("低鸣声"), None),
    (3, "s", 1, None, None, Some(PAST_OF_BLORF)),
    (14, "p1", 1, Some("to stop trying"), Some("放弃"), None),
    (14, "p2", 1, None, None, Some(SAME_AS_GIVE_UP)),
    (17, "s", 1, None, None, Some(SAME_AS_KOLOR)),
    (19, "s", 1, Some("a place"), Some("地方"), None),
    (19, "s", 2, None, None, Some(SAME_AS_KOLOR)),
];

/// key, entry, relation, kinds, via, group (in `ord` order per key).
type KeyRowData = (
    &'static str,
    i64,
    &'static str,
    Option<&'static str>,
    Option<&'static str>,
    Option<&'static str>,
);

#[rustfmt::skip]
const KEYS: &[KeyRowData] = &[
    ("blorf", 1, "headword", None, None, None),
    ("blorf", 2, "headword", None, None, None),
    ("blorft", 3, "headword", None, None, None),
    ("blorft", 1, "inflection", Some("p"), Some("blorf"), None),
    ("blorft", 2, "other", None, None, None),
    ("blorfs", 1, "inflection", Some("3"), Some("blorf"), None),
    ("blorfs", 2, "inflection", Some("s"), Some("blorf"), None),
    ("blorf out", 18, "headword", None, None, None),
    ("blorf out", 1, "phrasal", None, Some("ˌblorf ˈout"), Some("p1")),
    ("blorfout", 18, "headword", None, None, None),
    ("blorfout", 1, "phrasal", None, Some("ˌblorf ˈout"), Some("p1")),
    ("blorf the snib", 1, "idiom", None, Some("blorf the snib"), Some("i1")),
    ("blorfer", 1, "derivative", None, Some("blorfer"), Some("d1")),
    ("snib", 4, "headword", None, None, None),
    ("snib", 5, "inflection", Some("p"), Some("snab"), None),
    ("snib", 1, "idiom_part", None, Some("blorf the snib"), Some("i1")),
    ("the snib", 1, "idiom_part", None, Some("blorf the snib"), Some("i1")),
    ("glomp", 6, "headword", None, None, None),
    ("glomp", 7, "inflection", Some("p"), Some("glimp"), None),
    ("glomp", 10, "variant", None, Some("glomp"), None),
    ("gaolf", 17, "headword", None, None, None),
    ("gaolf", 10, "variant", None, Some("gaolf"), None),
    ("areb", 19, "headword", None, None, None),
    ("areb", 10, "variant", None, Some("areb"), None),
    ("Zorb", 8, "headword", None, None, None),
    ("zorb", 9, "headword", None, None, None),
    ("kolour", 10, "variant", None, Some("kolour"), None),
    ("launch", 11, "headword", None, None, None),
    ("launched", 11, "other", None, None, None),
    ("improve", 12, "headword", None, None, None),
    ("improving", 12, "inflection", Some("i"), Some("improve"), None),
    ("good", 13, "headword", None, None, None),
    ("better", 13, "inflection", Some("r"), Some("good"), None),
    ("give", 14, "headword", None, None, None),
    ("give up", 14, "phrasal", None, Some("give sth↔up"), Some("p1")),
    ("large", 14, "phrasal", None, Some("give large"), Some("p2")),
    ("snib blorf", 15, "headword", None, None, None),
    ("wug", 16, "headword", None, None, None),
    ("wugs", 16, "other", None, None, None),
    ("models", 16, "other", None, None, None),
    ("KLOMPS", 20, "headword", None, None, None),
    ("klomps", 21, "inflection", Some("s"), Some("klomp"), None),
    ("klomp", 21, "headword", None, None, None),
    ("snorb", 22, "headword", None, None, None),
    ("glimp", 7, "headword", None, None, None),
    ("glimp", 23, "headword", None, None, None),
];

fn oxford(dir: &Path) -> ExtDictionary {
    let path = dir.join("oalecd10.db");
    let conn = Connection::open(&path).unwrap();
    conn.execute_batch(SCHEMA).unwrap();
    conn.execute_batch(&format!(
        "INSERT INTO meta VALUES ('format', '{FORMAT}'), ('format_version', '{FORMAT_VERSION}'),
         ('source', 'oalecd10');"
    ))
    .unwrap();
    for (id, headword, pos, cefr, oxford, phon) in ENTRIES {
        conn.execute(
            "INSERT INTO entries (id, source_entry, entry_key, headword, pos, phon_gb, phon_us,
             cefr, oxford, ord) VALUES (?1, ?1, ?2, ?3, ?4, ?5, ?5, ?6, ?7, 0)",
            params![
                id,
                format!("{headword}||{pos}"),
                headword,
                Some(pos).filter(|p| !p.is_empty()),
                phon,
                cefr,
                oxford
            ],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO groups (entry_id, grp, ord) VALUES (?1, 's', 0)",
            [id],
        )
        .unwrap();
    }
    for (ord, (entry, grp, label)) in GROUPS.iter().enumerate() {
        conn.execute(
            "INSERT INTO groups (entry_id, grp, ord, label, fingerprint) VALUES (?1, ?2, ?3, ?4, 'f')",
            params![entry, grp, ord as i64 + 1, label],
        )
        .unwrap();
    }
    for (entry, grp, n, en, zh, xrefs) in SENSES {
        conn.execute(
            "INSERT INTO senses (entry_id, grp, sense, def_en, def_zh, xrefs, fingerprint)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, 'f')",
            params![entry, grp, n, en, zh, xrefs],
        )
        .unwrap();
    }
    conn.execute_batch(
        "INSERT INTO examples (entry_id, grp, sense, ord, en, zh)
         VALUES (1, 's', 1, 1, 'The engine blorfed.', '发动机低鸣。');
         INSERT INTO forms (entry_id, ord, kind, text) VALUES (1, 1, 'thirdps', 'blorfs');
         INSERT INTO variants (entry_id, ord, label, text) VALUES (1, 1, 'also', 'blorph');",
    )
    .unwrap();
    let mut last = "";
    let mut ord = 0;
    for (key, entry, relation, kinds, via, grp) in KEYS {
        ord = if key.eq_ignore_ascii_case(last) {
            ord + 1
        } else {
            0
        };
        last = key;
        conn.execute(
            "INSERT INTO keys (key, key_exact, entry_id, relation, kinds, via, grp, ord)
             VALUES (?1, ?1, ?2, ?3, ?4, ?5, ?6, ?7)",
            params![key, entry, relation, kinds, via, grp, ord],
        )
        .unwrap();
    }
    conn.execute_batch(INDEXES).unwrap();
    drop(conn);
    ExtDictionary::open(&path).unwrap()
}

struct Fixture {
    _dir: tempfile::TempDir,
    ecdict: Dictionary,
    oxford: ExtDictionary,
}

impl Fixture {
    fn new() -> Self {
        let dir = tempfile::tempdir().unwrap();
        Self {
            ecdict: fixture(dir.path()),
            oxford: oxford(dir.path()),
            _dir: dir,
        }
    }

    fn lexicon(&self) -> Lexicon<'_> {
        Lexicon {
            ecdict: &self.ecdict,
            ext: Some(&self.oxford),
        }
    }

    fn look(&self, text: &str) -> Merged {
        self.look_with(text, false)
    }

    fn look_with(&self, text: &str, sentence_start: bool) -> Merged {
        self.lexicon()
            .lookup_with(text, LookupOptions { sentence_start })
            .unwrap()
            .unwrap_or_else(|| panic!("{text} not found"))
    }
}

/// Each hit as `<source>:<word>[ <pos>] <role>[ (<relation>)][ open <group>]`.
fn hits(merged: &Merged) -> Vec<String> {
    merged
        .hits
        .iter()
        .map(|h| {
            let mut s = match &h.content {
                Content::Ecdict(e) => format!("E:{}", e.word),
                Content::Oxford(o) => format!("O:{} {}", o.entry.headword, o.entry.pos.join(",")),
            };
            s.push_str(&format!(" {:?}", h.role));
            if let Some(r) = &h.relation {
                s.push_str(&format!(" ({r})"));
            }
            if let Some(g) = &h.open_group {
                s.push_str(&format!(" open {g}"));
            }
            s
        })
        .collect()
}

#[test]
fn headword_entries_in_key_order_with_ecdict_tags() {
    let f = Fixture::new();
    let m = f.look("launch");
    assert_eq!(hits(&m), ["O:launch verb Query"]);
    let Content::Oxford(o) = &m.hits[0].content else {
        panic!("oxford");
    };
    // ECDICT's tags and, as the Oxford entry has no phonetic, its phonetic (6.4).
    assert_eq!(o.tags, ["cet4", "cet6"]);
    assert_eq!(o.ecdict_phonetic.as_deref(), Some("lɔ:ntʃ"));
    assert_eq!(m.lemma, "launch");
    assert!(m.notes().is_empty());

    let m = f.look("blorf");
    assert_eq!(hits(&m), ["O:blorf verb Query", "O:blorf noun Query"]);
    let Content::Oxford(o) = &m.hits[0].content else {
        panic!("oxford");
    };
    assert_eq!(o.ecdict_phonetic, None);
}

#[test]
fn the_selected_case_first_and_lowercase_first_at_a_sentence_start() {
    let f = Fixture::new();
    // An entry written as selected only in another case is not the key's own: last.
    assert_eq!(
        hits(&f.look("Zorb")),
        ["O:Zorb noun Query", "O:zorb verb OtherCase"]
    );
    assert_eq!(
        hits(&f.look("zorb")),
        ["O:zorb verb Query", "O:Zorb noun OtherCase"]
    );
    // At a sentence start both spellings count as written as selected, lowercase first.
    assert_eq!(
        hits(&f.look_with("Zorb", true)),
        ["O:zorb verb Query", "O:Zorb noun Query"]
    );
    // Neither written as selected: both, lowercase first (`m1-dict.md` 8).
    assert_eq!(
        hits(&f.look("ZORB")),
        ["O:zorb verb Query", "O:Zorb noun Query"]
    );
}

#[test]
fn rows_in_another_case_do_not_count_when_some_are_written_as_selected() {
    let f = Fixture::new();
    // `KLOMPS` is not the plural of klomp, whatever is selected (6.3 case a).
    let m = f.look("KLOMPS");
    assert_eq!(hits(&m), ["O:KLOMPS abbreviation Query"]);
    let m = f.look("klomps");
    assert_eq!(
        hits(&m),
        [
            "O:klomp noun Lemma (klomp 的复数)",
            "O:KLOMPS abbreviation OtherCase"
        ]
    );
    assert_eq!(m.notes(), ["klomps 是 klomp 的复数"]);
    assert_eq!(m.lemma, "klomp");
    // At a sentence start the lowercase spelling counts as written as selected (D046).
    assert_eq!(hits(&f.look_with("Klomps", true)), hits(&m));
    // Nothing written as selected: all rows, the own entry not marked as a form (case c).
    assert_eq!(
        hits(&f.look("Klomps")),
        [
            "O:klomp noun Lemma (klomp 的复数)",
            "O:KLOMPS abbreviation Query"
        ]
    );
}

#[test]
fn prefixes_and_suffixes_only_when_nothing_else_is_found() {
    let f = Fixture::new();
    assert_eq!(hits(&f.look("glimp")), ["O:glimp verb Query"]);
    assert_eq!(hits(&f.look("snorb")), ["O:-snorb suffix Related"]);
}

#[test]
fn relations_merge_into_one_text() {
    let forms = |base: &str, kinds: &[&str]| {
        Link::new(
            base,
            How::Forms(
                kinds
                    .iter()
                    .map(|k| KIND_ORDER.iter().position(|o| o == k).unwrap())
                    .collect(),
            ),
        )
    };
    // better: no kind for the adverb well, comparative for the adjective.
    assert_eq!(
        merge_links(&[
            forms("well", &[]),
            forms("well", &["r"]),
            forms("good", &["r"])
        ]),
        "well、good 的比较级"
    );
    assert_eq!(
        merge_links(&[forms("bad", &["r"]), forms("badly", &[])]),
        "bad 的比较级；badly 的变化形式"
    );
    assert_eq!(
        merge_links(&[forms("ward", &["s"]), forms("ward", &["3"])]),
        "ward 的第三人称单数、复数"
    );
    assert_eq!(
        merge_links(&[
            forms("ˈmeans-test", &["p", "d"]),
            forms("means-test", &["d", "p"])
        ]),
        "means-test 的过去式、过去分词"
    );
    assert_eq!(
        merge_links(&[
            forms("M", &["s"]),
            Link::new("manuscript", How::Variant),
            Link::new("MSc", How::Variant),
            Link::new("drily", How::Derivative),
        ]),
        "M 的复数；manuscript、MSc 的另一种写法；drily 的派生词"
    );
}

#[test]
fn own_entry_and_base_form_ordered_by_marks() {
    let f = Fixture::new();
    // `snib` is in the Oxford 3000: its own entry first. The idiom it is part of is not
    // shown, since it has an entry of its own.
    let m = f.look("snib");
    assert_eq!(
        hits(&m),
        [
            "O:snib noun FormOf (snab 的过去式)",
            "O:snab verb Lemma (snab 的过去式)"
        ]
    );
    assert_eq!(m.lemma, "snab");
    assert!(m.notes().is_empty());
    // `glomp` has no mark: the base form first.
    assert_eq!(
        hits(&f.look("glomp")),
        [
            "O:glimp verb Lemma (glimp 的过去式)",
            "O:glomp noun FormOf (glimp 的过去式)"
        ]
    );
}

#[test]
fn a_form_note_entry_gives_way_to_its_base_form() {
    let f = Fixture::new();
    // `blorft` only says "past tense of blorf": like `went`, only the base form.
    let m = f.look("blorft");
    assert_eq!(hits(&m), ["O:blorf verb Lemma (blorf 的过去式)"]);
    assert_eq!(m.notes(), ["blorft 是 blorf 的过去式"]);
    assert_eq!(m.lemma, "blorf");
}

#[test]
fn several_base_forms_each_with_a_note() {
    let f = Fixture::new();
    let m = f.look("blorfs");
    assert_eq!(
        hits(&m),
        [
            "O:blorf verb Lemma (blorf 的第三人称单数)",
            "O:blorf noun Lemma (blorf 的复数)"
        ]
    );
    // One line (6.3).
    assert_eq!(m.notes(), ["blorfs 是 blorf 的第三人称单数、复数"]);
}

#[test]
fn variants_derivatives_idioms_and_phrasal_verbs() {
    let f = Fixture::new();
    let m = f.look("kolour");
    assert_eq!(hits(&m), ["O:kolor noun Lemma (kolor 的另一种写法)"]);
    assert_eq!(m.lemma, "kolor");
    let m = f.look("blorfer");
    assert_eq!(hits(&m), ["O:blorf verb Lemma (blorf 的派生词) open d1"]);
    assert_eq!(m.notes(), ["blorfer 是 blorf 的派生词"]);
    let m = f.look("blorf the snib");
    assert_eq!(
        hits(&m),
        ["O:blorf verb Lemma (blorf 词条中的习语) open i1"]
    );
    assert_eq!(m.notes(), ["blorf the snib 是 blorf 词条中的习语"]);
    assert_eq!(m.lemma, "blorf the snib");
    // Only part of an idiom, and no entry of its own: the idiom.
    assert_eq!(
        hits(&f.look("the snib")),
        ["O:blorf verb Lemma (blorf 词条中的习语) open i1"]
    );
}

#[test]
fn a_whole_phrasal_verb_is_shown_next_to_an_entry_of_its_own() {
    let f = Fixture::new();
    let m = f.look("blorf out");
    assert_eq!(m.lemma, "blorf-out");
    // Written as selected: `blorf out` is the phrasal verb's spelling, not the noun's.
    assert_eq!(
        hits(&m),
        [
            "O:blorf verb Lemma (blorf 词条中的短语动词) open p1",
            "O:blorf-out noun Query"
        ]
    );
    // Neither is written `blorfout`: the entry of its own first.
    assert_eq!(
        hits(&f.look("blorfout")),
        [
            "O:blorf-out noun Query",
            "O:blorf verb Lemma (blorf 词条中的短语动词) open p1"
        ]
    );
}

#[test]
fn a_variant_with_an_entry_of_its_own_is_just_that_entry() {
    let f = Fixture::new();
    // `glomp` is also a variant of `kolor`: not shown, and not a form of it.
    let m = f.look("glomp");
    assert_eq!(
        hits(&m),
        [
            "O:glimp verb Lemma (glimp 的过去式)",
            "O:glomp noun FormOf (glimp 的过去式)"
        ]
    );
    // Unless its own entry says "= kolor".
    let m = f.look("gaolf");
    assert_eq!(
        hits(&m),
        [
            "O:kolor noun Lemma (kolor 的另一种写法)",
            "O:gaolf noun FormOf (kolor 的另一种写法)"
        ]
    );
    assert_eq!(m.lemma, "kolor");
    // An entry with definitions and one sense "= kolor": kolor after it, not a form of it.
    let m = f.look("areb");
    assert_eq!(
        hits(&m),
        [
            "O:areb noun Query",
            "O:kolor noun Lemma (kolor 的另一种写法)"
        ]
    );
    assert_eq!(m.lemma, "areb");
    assert!(m.notes().is_empty());
}

#[test]
fn ecdict_own_entry_when_oxford_has_the_key_only_as_a_form() {
    let f = Fixture::new();
    // `better` has exam tags: ECDICT's own entry first, with Oxford's relation.
    assert_eq!(
        hits(&f.look("better")),
        [
            "E:better FormOf (good 的比较级)",
            "O:good adjective Lemma (good 的比较级)"
        ]
    );
    // `improving` has no mark: the base form first.
    assert_eq!(
        hits(&f.look("improving")),
        [
            "O:improve verb Lemma (improve 的现在分词)",
            "E:improving FormOf (improve 的现在分词)"
        ]
    );
    // `give up` is a whole phrasal verb in Oxford: that, and not ECDICT's entry.
    let m = f.look("give up");
    assert_eq!(
        hits(&m),
        ["O:give verb Lemma (give 词条中的短语动词) open p1"]
    );
    assert_eq!(m.notes(), ["give up 是 give 词条中的短语动词"]);
    // A phrasal verb that only says "= give up": ECDICT's entry too.
    assert_eq!(
        hits(&f.look("large")),
        [
            "E:large Query",
            "O:give verb Lemma (give 词条中的短语动词) open p2"
        ]
    );
}

#[test]
fn keys_with_only_other_relations_go_through_ecdict() {
    let f = Fixture::new();
    // ECDICT reduces `launched` to `launch`, which Oxford has.
    let m = f.look("launched");
    assert_eq!(
        hits(&m),
        ["O:launch verb Lemma (launch 的过去式、过去分词)"]
    );
    assert_eq!(m.lemma, "launch");
    // ECDICT reduces `models` to `model` (suffix rule, M1), which only ECDICT has.
    assert_eq!(
        hits(&f.look("models")),
        ["E:model Lemma (model 的第三人称单数、复数)"]
    );
    // Nothing else: the `other` entry.
    let m = f.look("wugs");
    assert_eq!(hits(&m), ["O:wug noun Related"]);
    assert_eq!(m.lemma, "wugs");
}

#[test]
fn words_only_ecdict_has_are_as_in_m1() {
    let f = Fixture::new();
    let without = Lexicon {
        ecdict: &f.ecdict,
        ext: None,
    };
    for word in [
        "transformer",
        "gave up",
        "give up",
        "optimizer",
        "The",
        "launched",
    ] {
        let ecdict = f.ecdict.lookup(word).unwrap().map(from_ecdict);
        assert_eq!(
            without.lookup_with(word, LookupOptions::default()).unwrap(),
            ecdict,
            "{word}"
        );
    }
    assert_eq!(hits(&f.look("transformer")), ["E:transformer Query"]);
    // ECDICT reduces `gave up` to `give up`, a whole phrasal verb in Oxford.
    assert_eq!(
        hits(&f.look("gave up")),
        ["O:give verb Lemma (give up 的变化形式) open p1"]
    );
    assert_eq!(
        f.lexicon()
            .lookup_with("qwzx", LookupOptions::default())
            .unwrap(),
        None
    );
}

#[test]
fn hyphens_spaces_and_parts() {
    let f = Fixture::new();
    assert_eq!(hits(&f.look("snib-blorf")), ["O:snib blorf noun Query"]);
    let Some(MergedFound::Parts {
        parts, not_found, ..
    }) = f
        .lexicon()
        .lookup_with_parts("blorf-to-model-qwzx", LookupOptions::default())
        .unwrap()
    else {
        panic!("parts");
    };
    let first: Vec<String> = parts.iter().map(|p| hits(p)[0].clone()).collect();
    assert_eq!(first, ["O:blorf verb Query", "E:model Query"]);
    assert_eq!(not_found, ["qwzx"]);
}

#[test]
fn oxford_keys_are_words_and_phrases() {
    let f = Fixture::new();
    let lex = f.lexicon();
    assert!(lex.is_word("blorft").unwrap());
    assert!(lex.is_word("launch").unwrap());
    assert!(!lex.is_word("qwzx").unwrap());
    assert_eq!(
        lex.classify("blorf the snib").unwrap(),
        Classification::English(lextrail_core::lookup::LookupKind::Phrase)
    );
}

#[test]
fn whole_entries_are_read() {
    let f = Fixture::new();
    let e = f.oxford.entry(1).unwrap();
    assert_eq!(e.id_prefix, "oalecd10/blorf||verb");
    assert_eq!(e.cefr.as_deref(), Some("B1"));
    let groups: Vec<&str> = e.groups.iter().map(|g| g.id.as_str()).collect();
    assert_eq!(
        groups,
        [
            "oalecd10/blorf||verb/s",
            "oalecd10/blorf||verb/i1",
            "oalecd10/blorf||verb/p1",
            "oalecd10/blorf||verb/d1"
        ]
    );
    let main = e.group("s").unwrap();
    assert_eq!(main.senses[1].id, "oalecd10/blorf||verb/s/2");
    assert_eq!(
        main.senses[0].examples[0].zh.as_deref(),
        Some("发动机低鸣。")
    );
    assert!(main.senses[1].examples.is_empty());
    assert_eq!(e.forms[0].text, "blorfs");
    assert_eq!(e.variants[0].text, "blorph");
    assert!(!e.is_form_note());
    let note = f.oxford.entry(3).unwrap();
    assert!(note.is_form_note());
    assert!(note.is_form_note_of("blorf"));
    assert_eq!(note.groups[0].senses[0].xrefs[0].targets, ["blorf1"]);
}

#[test]
fn files_that_are_missing_or_of_another_format_are_refused() {
    let dir = tempfile::tempdir().unwrap();
    assert!(matches!(
        ExtDictionary::open(&dir.path().join("none.db")),
        Err(DictError::NotFound(_))
    ));
    let path = dir.path().join("old.db");
    let conn = Connection::open(&path).unwrap();
    conn.execute_batch(
        "CREATE TABLE meta (key TEXT PRIMARY KEY, value TEXT);
         INSERT INTO meta VALUES ('format', 'lextrail-ext-dict'), ('format_version', '0');",
    )
    .unwrap();
    drop(conn);
    assert!(matches!(
        ExtDictionary::open(&path),
        Err(DictError::UnsupportedFormat { .. })
    ));
    // The ECDICT dictionary is not an extended one.
    fixture(dir.path());
    assert!(matches!(
        ExtDictionary::open(&dir.path().join("dict.db")),
        Err(DictError::UnsupportedFormat { .. })
    ));
}
