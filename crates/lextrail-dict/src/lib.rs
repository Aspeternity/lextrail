//! LexTrail dictionary access, lemmatization and sense splitting.
//!
//! [`Dictionary`] reads the dictionary file and looks words and phrases up, reducing
//! inflected forms to their base form (ECDICT `exchange` data, then suffix rules).
//!
//! The dictionary file is built from ECDICT by `tools/dict-builder` (D038); its format is
//! in `docs/specs/m1-dict.md`. The parsing helpers here are shared with the builder so the
//! rules exist only once.

mod dictionary;
mod exchange;
pub mod schema;
mod senses;
mod suffix;

pub use dictionary::{
    DictError, Dictionary, Entry, EntryRole, Found, FoundEntry, Inflection, InflectionSource,
    LookupResult, PartsResult, normalize_query,
};
pub use exchange::{Exchange, FormKind, parse_exchange};
pub use senses::{Sense, split_translation};

/// Version of the dictionary file format (`meta.format_version`). A reader refuses files
/// with a version it does not know.
pub const FORMAT_VERSION: u32 = 1;

/// File name of the built-in compact dictionary.
pub const CORE_DICT_FILE_NAME: &str = "lextrail-dict-core.db";

/// Normalizes an ECDICT phonetic: the Cyrillic `ә` (U+04D9) used in about 100,000 entries
/// becomes the IPA schwa `ə` (U+0259). Everything else is kept as is.
pub fn normalize_phonetic(raw: &str) -> String {
    raw.trim().replace('\u{04D9}', "\u{0259}")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn phonetic_schwa_is_normalized() {
        assert_eq!(normalize_phonetic(" 'riәl "), "'riəl");
        assert_eq!(normalize_phonetic("ˈmju:zikəl"), "ˈmju:zikəl");
    }
}
