//! ECDICT phonetics rewritten in the notation the Oxford dictionary uses
//! (`docs/specs/m1.5-oxford.md` section 8). Only the notation changes: where the two
//! dictionaries transcribe a word differently (stress, weak vowels), ECDICT's transcription
//! is kept.

const VOWELS: &[char] = &[
    'a', 'e', 'i', 'o', 'u', 'æ', 'ɑ', 'ɒ', 'ɔ', 'ʌ', 'ɜ', 'ɪ', 'ʊ', 'ə',
];

fn is_vowel(c: char) -> bool {
    VOWELS.contains(&c)
}

/// Normalizes an ECDICT phonetic to the Oxford notation:
///
/// 1. the Cyrillic `ә` (U+04D9) becomes the IPA schwa `ə`;
/// 2. `'` → `ˈ`, `,` and `.` → `ˌ`, `:` → `ː`, `g` and `^` → `ɡ`, `ɛ` `є` `ε` → `e`;
/// 3. `əː` → `ɜː`, `ɒː` → `ɔː`;
/// 4. a short `i` / `u` becomes `ɪ` / `ʊ`, except at the end of a word and before another
///    vowel (the "happy" vowel); before `ə` it becomes `ɪə` / `ʊə` only in a stressed
///    syllable (here, tour), not in an unstressed one (media, actual);
/// 5. a short `ɔ` becomes `ɒ`, except before `i` or another vowel (`ɔɪ`); `ɒi` (boy,
///    oil in some entries) becomes `ɔɪ` too.
pub fn normalize_phonetic(raw: &str) -> String {
    let mapped: String = raw
        .trim()
        .chars()
        .map(|c| match c {
            '\u{04D9}' => 'ə',
            '\'' => 'ˈ',
            ',' | '.' => 'ˌ',
            ':' => 'ː',
            'g' | '^' => 'ɡ',
            'ɛ' | 'є' | 'ε' => 'e',
            c => c,
        })
        .collect();
    let chars: Vec<char> = mapped
        .replace("əː", "ɜː")
        .replace("ɒː", "ɔː")
        .chars()
        .collect();

    let mut out: Vec<char> = Vec::with_capacity(chars.len());
    for (i, &c) in chars.iter().enumerate() {
        let next = chars.get(i + 1).copied();
        let long = next == Some('ː');
        let converted = match c {
            'ɔ' if !long && !next.is_some_and(is_vowel) => 'ɒ',
            'ɒ' if next == Some('i') && chars.get(i + 2) != Some(&'ː') => 'ɔ',
            'i' | 'u' if !long => {
                let short = if c == 'i' { 'ɪ' } else { 'ʊ' };
                let after_vowel = out
                    .last()
                    .is_some_and(|&p| is_vowel(p) && p != 'i' && p != 'u');
                if after_vowel {
                    short
                } else if next == Some('ə') {
                    if stressed_syllable(&out) { short } else { c }
                } else if next.is_none_or(|n| n.is_whitespace() || is_vowel(n)) {
                    c
                } else {
                    short
                }
            }
            c => c,
        };
        out.push(converted);
    }
    out.into_iter().collect()
}

/// Whether the syllable being written is stressed: going back, a stress mark comes before
/// any vowel, or there is no vowel before at all.
fn stressed_syllable(written: &[char]) -> bool {
    for &c in written.iter().rev() {
        if c == 'ˈ' || c == 'ˌ' {
            return true;
        }
        if is_vowel(c) {
            return false;
        }
    }
    true
}

#[cfg(test)]
mod tests {
    use super::normalize_phonetic as n;

    #[test]
    fn symbols() {
        assert_eq!(n(" 'bætl "), "ˈbætl");
        assert_eq!(n("ˌæb'sent"), "ˌæbˈsent");
        assert_eq!(n(".kæt'wɔ:k"), "ˌkætˈwɔːk");
        assert_eq!(n("'^ræs"), "ˈɡræs");
        assert_eq!(n("gæp"), "ɡæp");
        for e in ["ɛ", "є", "ε"] {
            assert_eq!(n(&format!("b{e}d")), "bed");
        }
        assert_eq!(n("'riәl"), "ˈrɪəl");
    }

    #[test]
    fn long_vowels() {
        assert_eq!(n("bə:d"), "bɜːd");
        assert_eq!(n("kɒ:l"), "kɔːl");
        assert_eq!(n("kɔ:l"), "kɔːl");
        assert_eq!(n("si:d"), "siːd");
        assert_eq!(n("fu:d"), "fuːd");
    }

    #[test]
    fn short_i_and_u() {
        // elsewhere
        assert_eq!(n("bit"), "bɪt");
        assert_eq!(n("put"), "pʊt");
        // after a vowel: diphthongs
        assert_eq!(n("taim"), "taɪm");
        assert_eq!(n("geit"), "ɡeɪt");
        assert_eq!(n("bɔi"), "bɔɪ");
        assert_eq!(n("bɒi"), "bɔɪ");
        assert_eq!(n("haus"), "haʊs");
        assert_eq!(n("gəu"), "ɡəʊ");
        // end of a word, before another vowel: the "happy" vowel
        assert_eq!(n("'hæpi"), "ˈhæpi");
        assert_eq!(n("'hæpi 'deil"), "ˈhæpi ˈdeɪl");
        assert_eq!(n("'bliæk"), "ˈbliæk");
        assert_eq!(n("'influəns"), "ˈɪnfluəns");
    }

    #[test]
    fn i_and_u_before_schwa_by_stress() {
        // stressed, or no vowel before: centring diphthong
        assert_eq!(n("hiə"), "hɪə");
        assert_eq!(n("ai'diə"), "aɪˈdɪə");
        assert_eq!(n("tuə"), "tʊə");
        // unstressed syllable
        assert_eq!(n("'mi:diə"), "ˈmiːdiə");
        assert_eq!(n("'æktʃuəl"), "ˈæktʃuəl");
    }

    #[test]
    fn short_o() {
        assert_eq!(n("tɔp"), "tɒp");
        assert_eq!(n("bɔi"), "bɔɪ");
        assert_eq!(n("kɔ:t"), "kɔːt");
    }

    #[test]
    fn oxford_notation_is_kept() {
        for p in ["ˈhæpi", "ɡəʊ", "ˈmiːdiə", "bɜːd", "tɒp", "ˈɪnfluəns"] {
            assert_eq!(n(p), p);
        }
    }
}
