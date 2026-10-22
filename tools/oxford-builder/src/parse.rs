//! Parses one entry's HTML into fields (`docs/specs/m1.5-oxford.md` 0.3, 4, 5).
//!
//! Elements are located by class. Groups (idioms, phrasal verbs, derivatives) and senses
//! are numbered by their position among the source elements (5.1 rule 2, 3), whatever is
//! kept from them.

use std::collections::{BTreeMap, HashMap};

use ego_tree::NodeId;
use lextrail_dict::ext::id::{Group, GroupCounter, GroupKind};
use lextrail_dict::ext::text::{sense_fingerprint, text_hash};
use scraper::{ElementRef, Html};
use serde::Serialize;

use crate::html::{
    child, child_elements, collapse, find, has_any_class, has_class, non_empty, scoped, sounds,
    text, text_skipping,
};

/// A parsed entry.
#[derive(Debug, Clone, Default)]
pub struct Entry {
    /// `data-hkoup-entry`.
    pub source_entry: u32,
    /// Headword without syllable dots or the homograph number.
    pub headword: String,
    /// Other ways the headword is written: without a leading part such as "the" (`t-st`),
    /// the other headwords of a multi-headword entry.
    pub headword_alts: Vec<String>,
    pub homograph: String,
    pub pos: Vec<String>,
    pub phon_gb: Option<String>,
    pub phon_us: Option<String>,
    pub sound_gb: Option<String>,
    pub sound_us: Option<String>,
    pub cefr: Option<String>,
    /// 3000 or 5000 (the Oxford 3000 / 5000 key).
    pub oxford: Option<u32>,
    /// `W`, `S` or `W,S` (OPAL written / spoken).
    pub opal: Option<String>,
    /// The main group first, then idioms, phrasal verbs and derivatives in source order.
    pub groups: Vec<GroupData>,
    pub forms: Vec<Form>,
    pub variants: Vec<Variant>,
    pub notes: Vec<Note>,
    /// File names of the headword, form, idiom ... pronunciations (not of examples).
    pub sounds: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Xref {
    /// The `xt` attribute: `syn`, `opp`, `see`, `cp`, ...
    pub kind: String,
    /// The label as shown: "SYN", "see also", ...
    pub label: String,
    pub targets: Vec<String>,
}

#[derive(Debug, Clone)]
pub struct GroupData {
    pub group: Group,
    /// The idiom, phrasal verb or derivative as written (several joined by ` | `); `None`
    /// for the main senses.
    pub label: Option<String>,
    /// Each idiom / phrasal verb / derivative of the label separately, for matching keys.
    pub label_parts: Vec<String>,
    pub pos: Vec<String>,
    pub xrefs: Vec<Xref>,
    pub senses: Vec<SenseData>,
}

impl GroupData {
    /// The hash of the label (5.1 rule 5); `None` for the main senses.
    pub fn fingerprint(&self) -> Option<String> {
        self.label.as_deref().map(text_hash)
    }
}

#[derive(Debug, Clone, Default)]
pub struct SenseData {
    /// The sense number in its group (from 1).
    pub n: u32,
    pub cefr: Option<String>,
    pub gram: Option<String>,
    pub labels: Option<String>,
    pub hint_en: Option<String>,
    pub hint_zh: Option<String>,
    pub heading_en: Option<String>,
    pub heading_zh: Option<String>,
    pub patterns: Vec<String>,
    pub def_en: Option<String>,
    pub def_zh: Option<String>,
    pub xrefs: Vec<Xref>,
    pub examples: Vec<Example>,
    pub fingerprint: String,
}

#[derive(Debug, Clone, Default)]
pub struct Example {
    pub pattern: Option<String>,
    pub en: String,
    pub zh: Option<String>,
    pub sound_gb: Option<String>,
    pub sound_us: Option<String>,
}

#[derive(Debug, Clone)]
pub struct Form {
    pub group: Option<Group>,
    /// `thirdps`, `past`, ... (verb forms), or a code from the label of an irregular form
    /// (`s`, `r`, `t`, ...; see [`form_label_code`]); `None` when unlabelled.
    pub kind: Option<String>,
    pub text: String,
    /// From a verb form table (`t-vp`) rather than an irregular form (`t-if`).
    pub verb_form: bool,
    pub sound_gb: Option<String>,
    pub sound_us: Option<String>,
}

#[derive(Debug, Clone)]
pub struct Variant {
    pub group: Option<Group>,
    pub label: Option<String>,
    pub text: String,
}

#[derive(Debug, Clone)]
pub struct Note {
    pub group: Option<Group>,
    pub kind: Option<String>,
    pub text: String,
}

/// Counts for the report: elements by class, and fields that were expected but missing.
#[derive(Debug, Default, Clone, Serialize)]
pub struct ParseStats {
    /// Classes not in [`KNOWN_CLASSES`], with the number of elements.
    pub unknown_classes: BTreeMap<String, u64>,
    pub entries_without_pos: u64,
    pub entries_without_phon_gb: u64,
    pub entries_without_phon_us: u64,
    pub senses: u64,
    pub senses_without_definition: u64,
    pub senses_nested_in_senses: u64,
    pub groups_without_label: u64,
    pub examples_without_chinese: u64,
    pub examples_without_english: u64,
}

/// Every class of the 10th edition entries (`docs/specs/m1.5-oxford.md` 0.3): read into a
/// field, part of one, or deliberately ignored. Anything else is counted as unknown.
#[rustfmt::skip]
pub const KNOWN_CLASSES: &[&str] = &[
    "entry", "ill", "t-afrlabel", "t-alt", "t-alt-g", "t-arrow", "t-audio-afr", "t-audio-gb",
    "t-audio-us", "t-box-tab", "t-boxblock", "t-boxtag", "t-brelabel", "t-cf", "t-cf-blk",
    "t-chn", "t-chnsep", "t-chntd", "t-chnth", "t-cl", "t-cl-blk", "t-cl-header", "t-def",
    "t-def-qt", "t-dh", "t-dh-blk", "t-dis-g", "t-dr", "t-dr-blk", "t-dr-g", "t-dr-g-blk",
    "t-dr-gs", "t-dr-l", "t-drtri", "t-dtxt", "t-eb", "t-eb-blk", "t-ebi", "t-ei", "t-ei-blk",
    "t-em", "t-eng", "t-entry", "t-eph-blk", "t-exp", "t-fold", "t-form", "t-fthzindex",
    "t-ftindex", "t-geo", "t-geo-blk", "t-gl", "t-gl-blk", "t-gram", "t-gram-blk", "t-gram-g",
    "t-h", "t-h-g", "t-h-l", "t-hcomma", "t-hk", "t-hkey", "t-hm", "t-homonym", "t-hposbk",
    "t-id-g", "t-idm", "t-idm-blk", "t-idm-g", "t-idm-gs", "t-idm-gs-blk", "t-idm-l", "t-if",
    "t-if-blk", "t-if-g", "t-if-g-blk", "t-if-gs", "t-if-gs-blk", "t-ill", "t-ill-g", "t-index",
    "t-infl", "t-keybk", "t-label-g", "t-label-g-blk", "t-licontent", "t-namelabel", "t-ndv",
    "t-nil", "t-oalecd10", "t-or", "t-phon", "t-phon-blk", "t-pos", "t-pos-blk", "t-pos-g",
    "t-pron-g", "t-pron-g-blk", "t-pron-gs", "t-pronform", "t-pronfreq", "t-ptl", "t-pv",
    "t-pv-blk", "t-pv-g", "t-pv-g-blk", "t-pv-gs", "t-pv-gs-blk", "t-pv-l", "t-pvp-g",
    "t-pvp-g-blk", "t-q", "t-reg", "t-reg-blk", "t-res-g", "t-rx-g", "t-sdsymb", "t-shcut",
    "t-shcut-blk", "t-shcut-g-break", "t-sn-blk", "t-sn-blk-nolist", "t-sn-g", "t-sn-gs",
    "t-st", "t-subentry-g", "t-subj", "t-subj-blk", "t-symbol", "t-top-g", "t-topic",
    "t-topic-g", "t-topichead", "t-trans-g", "t-tw", "t-un", "t-unboxlink", "t-use",
    "t-use-blk", "t-v", "t-v-blk", "t-v-g", "t-v-g-blk", "t-v-gs", "t-v-gs-blk", "t-vbar",
    "t-ve", "t-vgslabel", "t-vp", "t-vp-g", "t-vp-gs", "t-vp-l", "t-vpform", "t-wx", "t-wx-blk",
    "t-x", "t-x-g-blk", "t-x-gs", "t-xh", "t-xh-blk", "t-xhm", "t-xpos", "t-xpos-blk", "t-xr-g",
    "t-xr-g-blk", "t-xr-gs", "t-xr-gs-inner", "t-xrlabel", "t-xs", "t-xs-blk", "t-xsymb",
    "t-xw", "t-xw-blk", "t-xwonly",
];

/// Group elements: an idiom, a phrasal verb, a derivative.
const GROUP_CLASSES: &[&str] = &["t-idm-g", "t-pv-g", "t-dr-g"];

fn group_kind(el: ElementRef) -> Option<GroupKind> {
    el.value().classes().find_map(|c| match c {
        "t-idm-g" => Some(GroupKind::Idiom),
        "t-pv-g" => Some(GroupKind::PhrasalVerb),
        "t-dr-g" => Some(GroupKind::Derivative),
        _ => None,
    })
}

/// Code of an irregular form's label (`t-form`): ECDICT's form codes where they exist.
pub fn form_label_code(label: &str) -> Option<&'static str> {
    Some(match label {
        "plural" => "s",
        "comparative" => "r",
        "superlative" => "t",
        "singular" => "sg",
        "feminine" => "fem",
        "negative" => "neg",
        "short form" => "short",
        _ => return None,
    })
}

fn strip_dots(s: &str) -> String {
    s.replace('·', "")
}

/// Where a sense's own fields end: nested groups and senses.
fn sense_wall(e: ElementRef) -> bool {
    has_any_class(e, &["t-idm-g", "t-id-g", "t-pv-g", "t-dr-g", "t-sn-g"])
}

/// Like [`sense_wall`], also leaving out examples, variants and cross-references.
fn sense_head_wall(e: ElementRef) -> bool {
    sense_wall(e) || has_any_class(e, &["t-x-gs", "t-v-gs-blk", "t-xr-gs", "t-un"])
}

/// Chinese and full-width characters, left out of a sense's English text (0.7 d).
fn is_chinese_or_full_width(c: char) -> bool {
    matches!(c,
        '\u{2E80}'..='\u{2FDF}' | '\u{3000}'..='\u{303F}' | '\u{3040}'..='\u{9FFF}'
        | '\u{F900}'..='\u{FAFF}' | '\u{FE30}'..='\u{FE4F}' | '\u{FF00}'..='\u{FFEF}'
        | '\u{20000}'..='\u{3FFFF}')
}

/// Parses an entry. `source_entry` is its `data-hkoup-entry` number.
pub fn parse(html: &str, source_entry: u32, stats: &mut ParseStats) -> anyhow::Result<Entry> {
    let doc = Html::parse_fragment(html);
    let root = doc
        .root_element()
        .descendants()
        .filter_map(ElementRef::wrap)
        .find(|e| has_class(*e, "entry"))
        .ok_or_else(|| anyhow::anyhow!("entry {source_entry}: no entry element"))?;

    for el in root.descendants().filter_map(ElementRef::wrap) {
        for class in el.value().classes() {
            if !KNOWN_CLASSES.contains(&class) {
                *stats.unknown_classes.entry(class.to_owned()).or_default() += 1;
            }
        }
    }

    let mut entry = Entry {
        source_entry,
        ..Entry::default()
    };
    head(root, &mut entry, source_entry)?;
    if entry.pos.is_empty() {
        stats.entries_without_pos += 1;
    }
    if entry.phon_gb.is_none() {
        stats.entries_without_phon_gb += 1;
    }
    if entry.phon_us.is_none() {
        stats.entries_without_phon_us += 1;
    }

    // Number the groups in source order.
    let mut counter = GroupCounter::default();
    let mut group_of: HashMap<NodeId, Group> = HashMap::new();
    let mut groups = vec![GroupData {
        group: Group::Main,
        label: None,
        label_parts: Vec::new(),
        pos: entry.pos.clone(),
        xrefs: Vec::new(),
        senses: Vec::new(),
    }];
    let mut index: HashMap<Group, usize> = HashMap::from([(Group::Main, 0)]);
    for el in root.descendants().filter_map(ElementRef::wrap) {
        if let Some(kind) = group_kind(el) {
            let group = counter.next(kind);
            group_of.insert(el.id(), group);
            index.insert(group, groups.len());
            groups.push(group_data(el, group, kind, stats));
        }
    }
    let enclosing = |el: ElementRef| -> Option<Group> {
        el.ancestors()
            .filter_map(ElementRef::wrap)
            .find(|a| has_any_class(*a, GROUP_CLASSES))
            .map(|a| group_of[&a.id()])
    };

    // Main group cross-references: outside senses and groups.
    groups[0].xrefs = xrefs(scoped(root, &|e| has_class(e, "t-xr-gs"), &|e| {
        sense_wall(e) || has_class(e, "t-x-gs")
    }));

    for el in root.descendants().filter_map(ElementRef::wrap) {
        if !has_class(el, "t-sn-g") {
            continue;
        }
        if el
            .ancestors()
            .filter_map(ElementRef::wrap)
            .take_while(|a| !has_any_class(*a, GROUP_CLASSES))
            .any(|a| has_class(a, "t-sn-g"))
        {
            stats.senses_nested_in_senses += 1;
        }
        let group = enclosing(el).unwrap_or(Group::Main);
        let data = &mut groups[index[&group]];
        let n = data.senses.len() as u32 + 1;
        data.senses.push(sense(el, n, stats));
    }

    for el in root.descendants().filter_map(ElementRef::wrap) {
        if has_class(el, "t-vp-g") {
            let Some(vp) = child(el, "t-vp") else {
                continue;
            };
            let (gb, us) = sounds(el, &|_| false);
            entry.forms.push(Form {
                group: enclosing(el),
                kind: el.value().attr("form").map(str::to_owned),
                text: strip_dots(&text(vp)),
                verb_form: true,
                sound_gb: gb,
                sound_us: us,
            });
        } else if has_class(el, "t-if") {
            let block = el
                .ancestors()
                .filter_map(ElementRef::wrap)
                .find(|a| has_class(*a, "t-if-g-blk"));
            let label = block
                .and_then(|b| child(b, "t-form"))
                .map(text)
                .unwrap_or_default();
            entry.forms.push(Form {
                group: enclosing(el),
                kind: form_label_code(&label).map(str::to_owned),
                text: strip_dots(&text(el)),
                verb_form: false,
                sound_gb: None,
                sound_us: None,
            });
        } else if has_class(el, "t-v-g") {
            let Some(v) = find(el, "t-v", &|_| false) else {
                continue;
            };
            let label = find(el, "t-vgslabel", &|_| false)
                .or_else(|| find(el, "t-label-g", &|_| false))
                .map(text)
                .and_then(non_empty);
            entry.variants.push(Variant {
                group: enclosing(el),
                label,
                text: strip_dots(&text(v)),
            });
        } else if has_class(el, "t-un") {
            entry.notes.push(Note {
                group: enclosing(el),
                kind: el.value().attr("un").map(str::to_owned),
                text: text(el),
            });
        }
    }
    entry.forms.retain(|f| !f.text.is_empty());
    entry.variants.retain(|v| !v.text.is_empty());

    // Pronunciations outside examples, for the audio file.
    for a in root.descendants().filter_map(ElementRef::wrap) {
        let Some(href) = a.value().attr("href") else {
            continue;
        };
        let Some(name) = href.strip_prefix("sound://") else {
            continue;
        };
        let pronunciation =
            child_elements(a).any(|c| has_any_class(c, &["t-audio-gb", "t-audio-us"]));
        let in_example = a
            .ancestors()
            .filter_map(ElementRef::wrap)
            .any(|p| has_class(p, "t-x"));
        if pronunciation && !in_example {
            entry.sounds.push(name.to_owned());
        }
    }
    entry.sounds.sort();
    entry.sounds.dedup();

    entry.groups = groups;
    Ok(entry)
}

/// Headword block: the parent of `t-hposbk` (`t-top-g`, or the entry itself in the other
/// layout, 0.3).
fn head(root: ElementRef, entry: &mut Entry, source_entry: u32) -> anyhow::Result<()> {
    let hposbk = find(root, "t-hposbk", &|_| false)
        .ok_or_else(|| anyhow::anyhow!("entry {source_entry}: no headword block"))?;
    let block = hposbk.parent().and_then(ElementRef::wrap).unwrap_or(root);
    let headwords = scoped(hposbk, &|e| has_class(e, "t-h"), &|_| false);
    let first = headwords
        .first()
        .ok_or_else(|| anyhow::anyhow!("entry {source_entry}: no headword"))?;
    let homograph = |e: ElementRef| has_any_class(e, &["t-hm", "t-homonym"]);
    entry.headword = strip_dots(&text_skipping(*first, &homograph));
    if entry.headword.is_empty() {
        anyhow::bail!("entry {source_entry}: empty headword");
    }
    entry.homograph = find(*first, "t-hm", &|_| false)
        .map(text)
        .unwrap_or_default();
    let mut alts = Vec::new();
    for h in &headwords {
        alts.push(strip_dots(&text_skipping(*h, &homograph)));
        alts.push(strip_dots(&text_skipping(*h, &|e| {
            homograph(e) || has_class(e, "t-st")
        })));
    }
    alts.retain(|a| !a.is_empty() && *a != entry.headword);
    alts.dedup();
    entry.headword_alts = alts;
    entry.pos = scoped(hposbk, &|e| has_class(e, "t-pos"), &|_| false)
        .into_iter()
        .map(text)
        .filter(|p| !p.is_empty())
        .collect();

    if let Some(prons) = child(block, "t-pron-gs") {
        for blk in child_elements(prons).filter(|e| has_class(*e, "t-pron-g-blk")) {
            let gb = child(blk, "t-brelabel").is_some();
            let us = child(blk, "t-namelabel").is_some();
            let phon = find(blk, "t-phon", &|_| false)
                .map(text)
                .and_then(non_empty);
            let (sound_gb, sound_us) = sounds(blk, &|_| false);
            if gb && entry.phon_gb.is_none() {
                entry.phon_gb = phon;
                entry.sound_gb = sound_gb;
            } else if us && entry.phon_us.is_none() {
                entry.phon_us = phon;
                entry.sound_us = sound_us;
            }
        }
    }

    if let Some(keys) = child(block, "t-keybk") {
        let typed = |t: &str| {
            keys.descendants()
                .filter_map(ElementRef::wrap)
                .filter(|e| e.value().attr("type") == Some(t))
                .map(text)
                .collect::<Vec<_>>()
        };
        if !typed("key").is_empty() {
            entry.oxford = Some(3000);
        } else if !typed("key2").is_empty() {
            entry.oxford = Some(5000);
        }
        entry.cefr = typed("cefr").into_iter().next();
        let opal = typed("opal");
        if !opal.is_empty() {
            entry.opal = Some(opal.join(","));
        }
    }
    Ok(())
}

fn group_data(el: ElementRef, group: Group, kind: GroupKind, stats: &mut ParseStats) -> GroupData {
    let (part, list) = match kind {
        GroupKind::Idiom => ("t-idm", "t-idm-l"),
        GroupKind::PhrasalVerb => ("t-pv", "t-pv-l"),
        GroupKind::Derivative => ("t-dr", "t-dr-l"),
    };
    let top = child(el, "t-top-g");
    let parts: Vec<String> = top
        .map(|t| scoped(t, &|e| has_class(e, part), &|_| false))
        .unwrap_or_default()
        .into_iter()
        .map(|e| strip_dots(&text(e)))
        .filter(|t| !t.is_empty())
        .collect();
    let label = match top.and_then(|t| find(t, list, &|_| false)) {
        Some(l) => non_empty(strip_dots(&text(l))),
        None => non_empty(parts.join(" | ")),
    };
    if label.is_none() {
        stats.groups_without_label += 1;
    }
    let pos = top
        .map(|t| {
            scoped(t, &|e| has_class(e, "t-pos"), &|e| {
                has_class(e, "t-v-gs-blk")
            })
            .into_iter()
            .map(text)
            .filter(|p| !p.is_empty())
            .collect()
        })
        .unwrap_or_default();
    let xrefs = xrefs(scoped(el, &|e| has_class(e, "t-xr-gs"), &|e| {
        sense_wall(e) || has_class(e, "t-x-gs")
    }));
    GroupData {
        group,
        label,
        label_parts: parts,
        pos,
        xrefs,
        senses: Vec::new(),
    }
}

fn xrefs(blocks: Vec<ElementRef>) -> Vec<Xref> {
    blocks
        .into_iter()
        .map(|b| Xref {
            kind: b.value().attr("xt").unwrap_or_default().to_owned(),
            label: child(b, "t-xrlabel").map(text).unwrap_or_default(),
            targets: scoped(b, &|e| has_class(e, "t-xr-g"), &|_| false)
                .into_iter()
                .map(text)
                .filter(|t| !t.is_empty())
                .collect(),
        })
        .collect()
}

fn eng_chn(el: ElementRef, wall: &dyn Fn(ElementRef) -> bool) -> (Option<String>, Option<String>) {
    let en = find(el, "t-eng", wall).map(text).and_then(non_empty);
    let zh = find(el, "t-chn", wall).map(text).and_then(non_empty);
    (en, zh)
}

fn sense(el: ElementRef, n: u32, stats: &mut ParseStats) -> SenseData {
    stats.senses += 1;
    let mut s = SenseData {
        n,
        ..SenseData::default()
    };
    // CEFR of a numbered sense: in the list item, before the sense element.
    if let Some(li) = el
        .parent()
        .and_then(ElementRef::wrap)
        .filter(|p| has_class(*p, "t-licontent"))
    {
        s.cefr = scoped(li, &|e| e.value().attr("type") == Some("cefr"), &|e| {
            has_class(e, "t-sn-g")
        })
        .into_iter()
        .next()
        .map(text);
    }
    s.gram = non_empty(
        scoped(el, &|e| has_class(e, "t-gram-g"), &sense_head_wall)
            .into_iter()
            .map(text)
            .collect::<Vec<_>>()
            .join(" "),
    );
    s.labels = non_empty(
        scoped(el, &|e| has_class(e, "t-label-g"), &sense_head_wall)
            .into_iter()
            .map(text)
            .collect::<Vec<_>>()
            .join("; "),
    );
    if let Some(dis) = find(el, "t-dis-g", &sense_head_wall) {
        (s.hint_en, s.hint_zh) = eng_chn(dis, &|_| false);
    }
    if let Some(heading) = heading(el) {
        (s.heading_en, s.heading_zh) = eng_chn(heading, &|_| false);
    }
    s.patterns = scoped(el, &|e| has_class(e, "t-cf"), &sense_head_wall)
        .into_iter()
        .map(text)
        .filter(|t| !t.is_empty())
        .collect();
    if let Some(def) = find(el, "t-def", &sense_head_wall) {
        (s.def_en, s.def_zh) = eng_chn(def, &sense_wall);
    }
    s.xrefs = xrefs(scoped(el, &|e| has_class(e, "t-xr-gs"), &|e| {
        sense_wall(e) || has_class(e, "t-x-gs")
    }));
    for gs in scoped(el, &|e| has_class(e, "t-x-gs"), &sense_wall) {
        for blk in scoped(gs, &|e| has_class(e, "t-x-g-blk"), &sense_wall) {
            let pattern = find(blk, "t-cf", &|e| has_class(e, "t-x"))
                .map(text)
                .and_then(non_empty);
            for x in scoped(blk, &|e| has_class(e, "t-x"), &sense_wall) {
                let (en, zh) = eng_chn(x, &|_| false);
                let (sound_gb, sound_us) = sounds(x, &|_| false);
                let Some(en) = en else {
                    stats.examples_without_english += 1;
                    continue;
                };
                if zh.is_none() {
                    stats.examples_without_chinese += 1;
                }
                s.examples.push(Example {
                    pattern: pattern.clone(),
                    en,
                    zh,
                    sound_gb,
                    sound_us,
                });
            }
        }
    }
    if s.def_en.is_none() {
        stats.senses_without_definition += 1;
    }
    let english: String = text_skipping(el, &|e| {
        has_class(e, "t-chn") || has_any_class(e, &["t-idm-g", "t-id-g"])
    })
    .chars()
    .filter(|&c| !is_chinese_or_full_width(c))
    .collect();
    s.fingerprint = sense_fingerprint(s.def_en.as_deref(), &collapse(&english));
    s
}

/// The heading (`t-shcut`) of the sense's part of the list: the nearest one before the
/// sense's list block.
fn heading(sense: ElementRef) -> Option<ElementRef> {
    let block = sense
        .ancestors()
        .filter_map(ElementRef::wrap)
        .take_while(|a| !has_any_class(*a, GROUP_CLASSES))
        .find(|a| has_any_class(*a, &["t-sn-blk", "t-sn-blk-nolist"]))?;
    let mut prev = block.prev_sibling();
    while let Some(node) = prev {
        if let Some(e) = ElementRef::wrap(node)
            && has_class(e, "t-shcut-blk")
        {
            return child(e, "t-shcut");
        }
        prev = node.prev_sibling();
    }
    None
}
