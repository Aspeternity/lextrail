//! LexTrail dictionary access, lemmatization and sense splitting.
//!
//! [`Dictionary`] reads the dictionary file and looks words and phrases up, reducing
//! inflected forms to their base form (ECDICT `exchange` data, then suffix rules).
//!
//! The dictionary file is built from ECDICT by `tools/dict-builder` (D038); its format is
//! in `docs/specs/m1-dict.md`. The parsing helpers here are shared with the builder so the
//! rules exist only once.

mod classify;
mod dictionary;
mod exchange;
pub mod ext;
mod merge;
mod phonetic;
pub mod schema;
mod senses;
mod suffix;
pub mod view;

pub use classify::{Classification, MAX_PHRASE_WORDS, classify};
pub use dictionary::{
    DictError, Dictionary, Entry, EntryRole, Found, FoundEntry, Inflection, InflectionSource,
    LookupOptions, LookupResult, PartsResult, normalize_query,
};
pub use exchange::{Exchange, FormKind, parse_exchange};
pub use merge::{Content, Hit, Lexicon, Merged, MergedFound, OxfordEntry, Role};
pub use phonetic::normalize_phonetic;
pub use senses::{Sense, split_translation};
pub use suffix::candidates as suffix_candidates;

/// Version of the dictionary file format (`meta.format_version`). A reader refuses files
/// with a version it does not know.
pub const FORMAT_VERSION: u32 = 2;

/// File name of the built-in compact dictionary.
pub const CORE_DICT_FILE_NAME: &str = "lextrail-dict-core.db";
