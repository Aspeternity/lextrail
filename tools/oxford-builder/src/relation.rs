//! Relation between a lookup key and an entry (`docs/specs/m1.5-oxford.md` 6.1), decided
//! once at conversion time: the first rule that holds, in the order of the spec's table.

use lextrail_dict::FormKind;
use lextrail_dict::ext::id::Group;
use lextrail_dict::ext::text::fold_for_match;

use crate::ecdict::Ecdict;
use crate::parse::Entry;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Relation {
    /// One of `lextrail_dict::ext::schema::RELATIONS`.
    pub relation: &'static str,
    /// Kinds of form (`inflection` only), in [`KIND_ORDER`].
    pub kinds: Vec<String>,
    /// The form in the entry the key matched or comes from: the base of an inflection,
    /// the variant, the derivative, the idiom. `None` for `headword` and `other`.
    pub via: Option<String>,
    /// The idiom / phrasal verb / derivative group the key leads to.
    pub group: Option<Group>,
}

/// Order of form kinds in `keys.kinds`: ECDICT's codes, then the ones only Oxford has.
pub const KIND_ORDER: &[&str] = &[
    "p", "d", "i", "3", "s", "r", "t", "neg", "short", "sg", "fem",
];

/// Verb form table (`t-vp-g[form]`) names to kind codes.
fn verb_form_code(form: &str) -> Option<&'static str> {
    Some(match form {
        "thirdps" => "3",
        "past" => "p",
        "pastpart" => "d",
        "prespart" => "i",
        "neg" => "neg",
        "short" => "short",
        _ => return None,
    })
}

fn sorted_kinds(mut kinds: Vec<String>) -> Vec<String> {
    kinds.sort_by_key(|k| KIND_ORDER.iter().position(|o| o == k).unwrap_or(usize::MAX));
    kinds.dedup();
    kinds
}

fn without_parenthesized(text: &str) -> String {
    let mut out = String::new();
    let mut depth = 0;
    for c in text.chars() {
        match c {
            '(' => depth += 1,
            ')' if depth > 0 => depth -= 1,
            c if depth == 0 => out.push(c),
            _ => {}
        }
    }
    out
}

/// Folded spellings of a form: as written, and without parenthesized parts.
fn folds(text: &str) -> [String; 2] {
    [
        fold_for_match(text),
        fold_for_match(&without_parenthesized(text)),
    ]
}

fn same(key: &str, text: &str) -> bool {
    folds(text).iter().any(|f| !f.is_empty() && f == key)
}

/// British / American spelling differences folded away (colour/color, centre/center,
/// organise/organize, travelling/traveling ...). Only used between a key and an entry the
/// source already links, so over-matching is harmless.
fn spelling(folded: &str) -> String {
    let mut t = folded.to_owned();
    for (a, b) in [
        ("isation", "ization"),
        ("ise", "ize"),
        ("yse", "yze"),
        ("our", "or"),
        ("tre", "ter"),
        ("ogue", "og"),
        ("ae", "e"),
        ("oe", "e"),
        ("ll", "l"),
        ("ence", "ense"),
        ("disc", "disk"),
    ] {
        t = t.replace(a, b);
    }
    t
}

fn words(text: &str) -> Vec<String> {
    // `↔` marks where an object can go: `play sth↔down`.
    text.split(|c: char| c.is_whitespace() || c == '/' || c == '↔')
        .map(fold_for_match)
        .filter(|w| !w.is_empty())
        .collect()
}

/// Words that stand for any noun phrase in idioms and phrasal verbs (`put sb off`).
const PLACEHOLDERS: &[&str] = &[
    "sb",
    "sth",
    "sbs",
    "sths",
    "somebody",
    "something",
    "someone",
    "oneself",
    "ones",
    "yourself",
    "myself",
    "himself",
    "herself",
    "itself",
    "themselves",
];

fn phrase_words(phrase: &str) -> Vec<String> {
    words(phrase)
        .into_iter()
        .filter(|w| !PLACEHOLDERS.contains(&w.as_str()))
        .collect()
}

/// The key is the phrase without its placeholders (`put off` or `putoff` for `put sb off`).
fn is_phrase(key: &str, phrase: &str) -> bool {
    let k = words(key);
    let p = phrase_words(phrase);
    !k.is_empty() && (k == p || k.concat() == p.concat())
}

/// The key's words appear together in the phrase (`break` in `not break the bank`).
fn part_of(key: &str, phrase: &str) -> bool {
    let k = words(key);
    let p = phrase_words(phrase);
    !k.is_empty() && p.len() > k.len() && p.windows(k.len()).any(|w| w == k.as_slice())
}

/// Whether a kind of form fits the base's parts of speech: a plural needs a noun (or a
/// pronoun, number, abbreviation, symbol), third person and the `-ed` / `-ing` forms a
/// verb, comparative and superlative an adjective, adverb, determiner or pronoun. A base without parts of speech takes all (`blorfer` is not a comparative of the
/// verb `blorf`; `found` is not a form of the noun `find`).
fn fits(kind: FormKind, pos: &[String]) -> bool {
    let has = |word: &str| {
        pos.iter()
            .any(|p| p.split(|c: char| !c.is_alphabetic()).any(|t| t == word))
    };
    pos.is_empty()
        || match kind {
            FormKind::Plural => ["noun", "pronoun", "number", "abbreviation", "symbol"]
                .iter()
                .any(|p| has(p)),
            FormKind::ThirdPerson
            | FormKind::Past
            | FormKind::PastParticiple
            | FormKind::PresentParticiple => has("verb"),
            FormKind::Comparative | FormKind::Superlative => {
                ["adjective", "adverb", "determiner", "pronoun"]
                    .iter()
                    .any(|p| has(p))
            }
        }
}

/// A possible base form of the key (folded), from ECDICT's forms or the suffix rules,
/// applied to the whole key, or to its last or first word (`bear markets`, `knock-downs`,
/// `maids of honour`).
struct KeyBase {
    folded: String,
    /// ECDICT kind codes, when ECDICT gives this base.
    ecdict: Vec<String>,
    /// Kinds by the suffix rules, when they give this base.
    suffix: &'static [FormKind],
}

impl KeyBase {
    /// The kinds that fit the base's parts of speech; ECDICT's when it gives this base.
    fn kinds(&self, pos: &[String]) -> Vec<String> {
        if self.ecdict.is_empty() {
            self.suffix
                .iter()
                .filter(|k| fits(**k, pos))
                .map(|k| k.code().to_owned())
                .collect()
        } else {
            self.ecdict
                .iter()
                .filter(|code| FormKind::from_code(code).is_none_or(|k| fits(k, pos)))
                .cloned()
                .collect()
        }
    }
}

/// Kinds of the key as a form of `text` (a headword, variant or derivative with parts of
/// speech `pos`), when one of its possible bases is `text` with fitting kinds.
fn form_of(key_bases: &[KeyBase], text: &str, pos: &[String]) -> Option<Vec<String>> {
    key_bases
        .iter()
        .filter(|b| same(&b.folded, text))
        .map(|b| b.kinds(pos))
        .find(|k| !k.is_empty())
}

fn key_bases(key: &str, ecdict: &Ecdict) -> Vec<KeyBase> {
    let mut out = Vec::new();
    let mut add = |prefix: &str, word: &str, suffix: &str| {
        for (lemma, kinds) in ecdict.lemmas(word) {
            out.push(KeyBase {
                folded: format!("{prefix}{}{suffix}", fold_for_match(&lemma)),
                ecdict: kinds,
                suffix: &[],
            });
        }
        for (base, kinds) in lextrail_dict::suffix_candidates(&fold_for_match(word)) {
            out.push(KeyBase {
                folded: format!("{prefix}{base}{suffix}"),
                ecdict: Vec::new(),
                suffix: kinds,
            });
        }
    };
    add("", key, "");
    let words: Vec<&str> = key
        .split(|c: char| c.is_whitespace() || c == '-')
        .filter(|w| !w.is_empty())
        .collect();
    if let [first, middle @ .., last] = words.as_slice() {
        let fold_all = |ws: &[&str]| ws.iter().map(|w| fold_for_match(w)).collect::<String>();
        let before_last = fold_all(&words[..words.len() - 1]);
        add(&before_last, last, "");
        let after_first = fold_all(&[middle, &[*last]].concat());
        add("", first, &after_first);
    }
    out
}

struct Base<'a> {
    text: &'a str,
    group: Option<Group>,
    pos: &'a [String],
}

/// Kinds of a verb form (third person, past, past participle, present participle).
const VERB_KINDS: &[&str] = &["3", "p", "d", "i"];

/// Inferred kinds without the verb ones when the base has its own verb form table that
/// does not list the key: the table lists the verb's forms, and ECDICT (which does not tell
/// homographs apart) would otherwise make `lay` a past tense of "lie" (to say what is not
/// true), whose table says `lied`.
fn without_verb_kinds(kinds: Vec<String>, table_says_otherwise: bool) -> Option<Vec<String>> {
    let kinds: Vec<String> = if table_says_otherwise {
        kinds
            .into_iter()
            .filter(|k| !VERB_KINDS.contains(&k.as_str()))
            .collect()
    } else {
        kinds
    };
    (!kinds.is_empty()).then_some(kinds)
}

pub fn relate(key: &str, entry: &Entry, ecdict: &Ecdict) -> Relation {
    let k = fold_for_match(key);
    let key_bases = key_bases(key, ecdict);
    let rel = |relation, kinds, via: Option<&str>, group| Relation {
        relation,
        kinds,
        via: via.map(str::to_owned),
        group,
    };
    let headwords: Vec<&str> = std::iter::once(entry.headword.as_str())
        .chain(entry.headword_alts.iter().map(String::as_str))
        .collect();
    if headwords.iter().any(|h| same(&k, h)) {
        return rel("headword", Vec::new(), None, None);
    }

    let base_pos = |group: Option<Group>| -> &[String] {
        group
            .and_then(|g| entry.groups.iter().find(|d| d.group == g))
            .map_or(&entry.pos, |d| &d.pos)
    };
    let base_of = |group: Option<Group>| -> &str {
        group
            .and_then(|g| entry.groups.iter().find(|d| d.group == g))
            .and_then(|d| d.label_parts.first())
            .map_or(entry.headword.as_str(), String::as_str)
    };

    // Forms written in the entry: verb form tables first, then irregular forms.
    for verb_forms in [true, false] {
        let matching: Vec<_> = entry
            .forms
            .iter()
            .filter(|f| f.verb_form == verb_forms && same(&k, &f.text))
            .collect();
        let Some(first) = matching.first() else {
            continue;
        };
        let base = base_of(first.group);
        let mut kinds: Vec<String> = matching
            .iter()
            .filter(|f| f.group == first.group)
            .filter_map(|f| f.kind.as_deref())
            .filter_map(|kind| {
                if verb_forms {
                    verb_form_code(kind).map(str::to_owned)
                } else {
                    Some(kind.to_owned())
                }
            })
            .collect();
        if kinds.is_empty() && !verb_forms {
            kinds = form_of(&key_bases, base, base_pos(first.group)).unwrap_or_default();
        }
        return rel("inflection", sorted_kinds(kinds), Some(base), first.group);
    }

    // Regular forms the entry does not write out: of the headword, a variant, a derivative.
    let mut bases: Vec<Base> = headwords
        .iter()
        .map(|h| Base {
            text: h,
            group: None,
            pos: &entry.pos,
        })
        .collect();
    for v in &entry.variants {
        bases.push(Base {
            text: &v.text,
            group: v.group,
            pos: v
                .group
                .and_then(|g| entry.groups.iter().find(|d| d.group == g))
                .map_or(&entry.pos, |d| &d.pos),
        });
    }
    for g in entry
        .groups
        .iter()
        .filter(|g| matches!(g.group, Group::Derivative(_)))
    {
        for part in &g.label_parts {
            bases.push(Base {
                text: part,
                group: Some(g.group),
                pos: &g.pos,
            });
        }
    }
    // The verb form table of `base` (its `root` is the base; a variant's forms are not in
    // the headword's table) lists other forms, and not the key in another spelling
    // (`colored` for `coloured`).
    let k_spelling = spelling(&k);
    let table_says_otherwise = |group: Option<Group>, base: &str| {
        let table: Vec<_> = entry
            .forms
            .iter()
            .filter(|f| f.verb_form && f.group == group)
            .collect();
        table
            .iter()
            .any(|f| f.kind.as_deref() == Some("root") && same(&fold_for_match(base), &f.text))
            && !table
                .iter()
                .any(|f| folds(&f.text).iter().any(|t| spelling(t) == k_spelling))
    };
    for base in &bases {
        if let Some(kinds) = form_of(&key_bases, base.text, base.pos)
            .and_then(|k| without_verb_kinds(k, table_says_otherwise(base.group, base.text)))
        {
            return rel(
                "inflection",
                sorted_kinds(kinds),
                Some(base.text),
                base.group,
            );
        }
    }

    if let Some(v) = entry.variants.iter().find(|v| same(&k, &v.text)) {
        return rel("variant", Vec::new(), Some(&v.text), v.group);
    }
    // British / American spelling of the headword, or of its regular forms.
    for h in &headwords {
        if folds(h)
            .iter()
            .any(|f| !f.is_empty() && spelling(f) == k_spelling)
        {
            return rel("variant", Vec::new(), Some(h), None);
        }
    }
    for h in &headwords {
        let h_spelling = spelling(&fold_for_match(h));
        let kinds = key_bases
            .iter()
            .filter(|b| spelling(&b.folded) == h_spelling)
            .map(|b| b.kinds(&entry.pos))
            .find_map(|k| without_verb_kinds(k, table_says_otherwise(None, h)));
        if let Some(kinds) = kinds {
            return rel("inflection", sorted_kinds(kinds), Some(h), None);
        }
    }

    for (kind, relation) in [
        (Group::Derivative(0), "derivative"),
        (Group::Idiom(0), "idiom"),
        (Group::PhrasalVerb(0), "phrasal"),
    ] {
        let same_kind = |g: Group| std::mem::discriminant(&g) == std::mem::discriminant(&kind);
        for g in entry.groups.iter().filter(|g| same_kind(g.group)) {
            if let Some(part) = g
                .label_parts
                .iter()
                .find(|p| same(&k, p) || is_phrase(key, p))
            {
                return rel(relation, Vec::new(), Some(part), Some(g.group));
            }
        }
    }
    // Part of a longer idiom or phrasal verb.
    for g in &entry.groups {
        let relation = match g.group {
            Group::Idiom(_) => "idiom_part",
            Group::PhrasalVerb(_) => "phrasal_part",
            _ => continue,
        };
        if let Some(part) = g.label_parts.iter().find(|p| part_of(key, p)) {
            return rel(relation, Vec::new(), Some(part), Some(g.group));
        }
    }
    rel("other", Vec::new(), None, None)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::parse::{Form, GroupData, Variant};

    fn group(group: Group, parts: &[&str], pos: &[&str]) -> GroupData {
        GroupData {
            group,
            label: Some(parts.join(" | ")),
            label_parts: parts.iter().map(|p| p.to_string()).collect(),
            pos: pos.iter().map(|p| p.to_string()).collect(),
            xrefs: Vec::new(),
            senses: Vec::new(),
        }
    }

    /// A made-up verb "blorf" with a derivative, an idiom and a phrasal verb.
    fn blorf() -> Entry {
        Entry {
            source_entry: 1,
            headword: "blorf".into(),
            headword_alts: vec![],
            pos: vec!["verb".into()],
            groups: vec![
                group(Group::Main, &[], &["verb"]),
                group(Group::Derivative(1), &["ˈblorfer"], &["noun"]),
                group(Group::Idiom(1), &["blorf the ˈsnib"], &[]),
                group(Group::PhrasalVerb(1), &["ˌblorf sb ˈout"], &[]),
            ],
            forms: vec![
                Form {
                    group: None,
                    kind: Some("root".into()),
                    text: "blorf".into(),
                    verb_form: true,
                    sound_gb: None,
                    sound_us: None,
                },
                Form {
                    group: None,
                    kind: Some("past".into()),
                    text: "blorft".into(),
                    verb_form: true,
                    sound_gb: None,
                    sound_us: None,
                },
                Form {
                    group: None,
                    kind: Some("pastpart".into()),
                    text: "blorft".into(),
                    verb_form: true,
                    sound_gb: None,
                    sound_us: None,
                },
                Form {
                    group: None,
                    kind: None,
                    text: "blorven".into(),
                    verb_form: false,
                    sound_gb: None,
                    sound_us: None,
                },
            ],
            variants: vec![Variant {
                group: None,
                label: Some("also".into()),
                text: "bloarf".into(),
            }],
            ..Entry::default()
        }
    }

    fn r(key: &str) -> Relation {
        relate(
            key,
            &blorf(),
            &Ecdict::with_forms(&[("blorven", "blorf", "d")]),
        )
    }

    #[test]
    fn headword_ignores_case_spaces_hyphens_and_stress() {
        for key in ["blorf", "Blorf", "BLORF", "blo-rf", "ˈblorf"] {
            assert_eq!(r(key).relation, "headword", "{key}");
        }
    }

    #[test]
    fn written_forms() {
        let found = r("blorft");
        assert_eq!(found.relation, "inflection");
        assert_eq!(found.kinds, ["p", "d"]);
        assert_eq!(found.via.as_deref(), Some("blorf"));
        // irregular form without a label: kind from ECDICT
        assert_eq!(r("blorven").kinds, ["d"]);
        // ECDICT's kinds must fit the entry's parts of speech
        let mut noun = blorf();
        noun.pos = vec!["noun".into()];
        noun.forms.clear();
        let ecdict = Ecdict::with_forms(&[
            ("blorft", "blorf", "p"),
            ("blorfs", "blorf", "3"),
            ("blorfs", "blorf", "s"),
        ]);
        assert_eq!(relate("blorft", &noun, &ecdict).relation, "other");
        assert_eq!(relate("blorfs", &noun, &ecdict).kinds, ["s"]);
    }

    /// [`r`] on blorf without its verb form table.
    fn r_untabled(key: &str) -> Relation {
        let mut e = blorf();
        e.forms.retain(|f| !f.verb_form);
        relate(key, &e, &Ecdict::with_forms(&[("blorven", "blorf", "d")]))
    }

    #[test]
    fn regular_forms_of_headword_variant_and_derivative() {
        let r = r_untabled;
        let s = r("blorfs");
        assert_eq!(
            (s.relation, s.kinds.clone()),
            ("inflection", vec!["3".to_owned()])
        );
        assert_eq!(r("blorfing").kinds, ["i"]);
        let v = r("bloarfed");
        assert_eq!(
            (v.relation, v.via.as_deref()),
            ("inflection", Some("bloarf"))
        );
        let d = r("blorfers");
        assert_eq!(d.relation, "inflection");
        assert_eq!(d.kinds, ["s"]);
        assert_eq!(d.group, Some(Group::Derivative(1)));
    }

    #[test]
    fn a_verb_form_table_stops_inferring_verb_forms() {
        // blorf's table lists its verb forms: `blorfing` is not inferred.
        assert_eq!(r("blorfing").relation, "other");
        // The table is blorf's, not its variant's.
        assert_eq!(r("bloarfed").kinds, ["p", "d"]);
        assert_eq!(r_untabled("blorfing").kinds, ["i"]);
        // Plurals of the derivative (no table in its group) still are.
        assert_eq!(r("blorfers").kinds, ["s"]);
    }

    #[test]
    fn variants_derivatives_idioms_phrasal_verbs() {
        assert_eq!(r("bloarf").relation, "variant");
        let d = r("blorfer");
        assert_eq!(
            (d.relation, d.group),
            ("derivative", Some(Group::Derivative(1)))
        );
        assert_eq!(d.via.as_deref(), Some("ˈblorfer"));
        assert_eq!(r("blorf the snib").relation, "idiom");
        assert_eq!(r("blorf sb out").relation, "phrasal");
        assert_eq!(r("blorfout").relation, "phrasal");
        assert_eq!(r("snib").relation, "idiom_part");
        let p = r("blorf out");
        assert_eq!(
            (p.relation, p.group),
            ("phrasal", Some(Group::PhrasalVerb(1)))
        );
        assert_eq!(r("zzz").relation, "other");
    }

    #[test]
    fn british_and_american_spellings() {
        let mut e = blorf();
        e.headword = "snibour".into();
        e.forms.retain(|f| !f.verb_form);
        let ecdict = Ecdict::default();
        assert_eq!(relate("snibor", &e, &ecdict).relation, "variant");
        let s = relate("snibors", &e, &ecdict);
        assert_eq!(
            (s.relation, s.via.as_deref()),
            ("inflection", Some("snibour"))
        );
    }

    #[test]
    fn parenthesized_parts_are_optional() {
        let mut e = blorf();
        e.headword = "snib(s)".into();
        assert_eq!(relate("snib", &e, &Ecdict::default()).relation, "headword");
        assert_eq!(relate("snibs", &e, &Ecdict::default()).relation, "headword");
    }
}
