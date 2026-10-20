//! Fallback lemmatization by English suffix rules, for word forms that ECDICT's `exchange`
//! field does not list (e.g. `transformers`). Only candidates found in the dictionary are
//! used, so the rules may over-generate.

use crate::FormKind;

/// Possible base forms of a lowercase word, most likely first, with the kinds of form the
/// word would then be. Words that are not plain ASCII letters give none.
pub fn candidates(word: &str) -> Vec<(String, &'static [FormKind])> {
    use FormKind::*;
    const S: &[FormKind] = &[Plural, ThirdPerson];
    const ED: &[FormKind] = &[Past, PastParticiple];
    const ING: &[FormKind] = &[PresentParticiple];
    const ER: &[FormKind] = &[Comparative];
    const EST: &[FormKind] = &[Superlative];

    let mut out: Vec<(String, &'static [FormKind])> = Vec::new();
    if word.len() < 4 || !word.bytes().all(|b| b.is_ascii_lowercase()) {
        return out;
    }
    let mut add = |stem: &str, tail: &str, kinds: &'static [FormKind]| {
        let base = format!("{stem}{tail}");
        if base.len() >= 3 && !out.iter().any(|(b, _)| *b == base) {
            out.push((base, kinds));
        }
    };
    // `-ies`, `-ied`, `-ier`, `-iest` -> `-y` (studies, studied, happier, happiest).
    for (suffix, kinds) in [("ies", S), ("ied", ED), ("ier", ER), ("iest", EST)] {
        if let Some(stem) = word.strip_suffix(suffix) {
            add(stem, "y", kinds);
        }
    }
    // `-ying` -> `-ie` (dying).
    if let Some(stem) = word.strip_suffix("ying") {
        add(stem, "ie", ING);
    }
    // `-es` after s, x, z, ch, sh (boxes); then plain `-s` (models), but not `-ss`.
    if let Some(stem) = word.strip_suffix("es")
        && (stem.ends_with(['s', 'x', 'z']) || stem.ends_with("ch") || stem.ends_with("sh"))
    {
        add(stem, "", S);
    }
    if let Some(stem) = word.strip_suffix('s')
        && !stem.ends_with('s')
    {
        add(stem, "", S);
    }
    for (suffix, kinds) in [("ed", ED), ("ing", ING), ("er", ER), ("est", EST)] {
        if let Some(stem) = word.strip_suffix(suffix) {
            // stopped -> stop, running -> run, bigger -> big.
            if let Some(single) = undouble(stem) {
                add(single, "", kinds);
            }
            // wanted -> want, reading -> read; used -> use, making -> make, larger -> large.
            add(stem, "", kinds);
            add(stem, "e", kinds);
        }
    }
    out
}

/// `stopp` -> `stop`: a doubled final consonant.
fn undouble(stem: &str) -> Option<&str> {
    let b = stem.as_bytes();
    let n = b.len();
    (n >= 3 && b[n - 1] == b[n - 2] && !b"aeiou".contains(&b[n - 1])).then(|| &stem[..n - 1])
}

#[cfg(test)]
mod tests {
    use super::*;

    fn bases(word: &str) -> Vec<String> {
        candidates(word).into_iter().map(|(b, _)| b).collect()
    }

    #[test]
    fn plurals_and_third_person() {
        assert_eq!(bases("models")[0], "model");
        assert_eq!(bases("studies")[0], "study");
        assert!(bases("boxes").contains(&"box".to_owned()));
        assert!(bases("matches").contains(&"match".to_owned()));
        // `-ss` is not a plural.
        assert!(!bases("class").contains(&"clas".to_owned()));
    }

    #[test]
    fn past_and_participles() {
        assert!(bases("wanted").contains(&"want".to_owned()));
        assert!(bases("used").contains(&"use".to_owned()));
        assert_eq!(bases("stopped")[0], "stop");
        assert_eq!(bases("studied")[0], "study");
        assert!(bases("reading").contains(&"read".to_owned()));
        assert!(bases("making").contains(&"make".to_owned()));
        assert_eq!(bases("running")[0], "run");
        assert_eq!(bases("dying")[0], "die");
    }

    #[test]
    fn comparatives() {
        assert_eq!(bases("bigger")[0], "big");
        assert!(bases("larger").contains(&"large".to_owned()));
        assert_eq!(bases("happiest")[0], "happy");
    }

    #[test]
    fn kinds_follow_the_suffix() {
        let c = candidates("models");
        assert_eq!(c[0].1, &[FormKind::Plural, FormKind::ThirdPerson]);
        let c = candidates("stopped");
        assert_eq!(c[0].1, &[FormKind::Past, FormKind::PastParticiple]);
    }

    #[test]
    fn short_or_non_ascii_words_have_none() {
        assert!(candidates("is").is_empty());
        assert!(candidates("café").is_empty());
        assert!(candidates("Models").is_empty());
        assert!(candidates("self-attention").is_empty());
    }

    #[test]
    fn no_duplicates() {
        let b = bases("seed");
        let mut d = b.clone();
        d.dedup();
        assert_eq!(b, d);
    }
}
