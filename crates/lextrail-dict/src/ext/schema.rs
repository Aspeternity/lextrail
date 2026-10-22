//! Tables of an extended dictionary file and of its audio file
//! (`docs/specs/m1.5-oxford.md` section 4), shared by the converter (writer) and the reader.
//!
//! Tables whose rows can be long (definitions, examples, notes) keep a rowid (in a
//! `WITHOUT ROWID` table a row over about 1/4 of a page spills into overflow pages) and
//! are read by `entry_id`; rows are inserted in entry, group and sense order, and
//! (`entry_id`, `grp`, `sense`[, `ord`]) is unique by construction.
//! Lists (cross-references, patterns) are JSON arrays in TEXT columns; groups and senses are
//! identified by `entry_id`, `grp` (the [`super::id::Group`] text: `s`, `i3`, ...) and the
//! sense number, so the stable id of a sense is `entries.entry_key` + `grp` + `sense`.

/// `meta.format` of an extended dictionary.
pub const FORMAT: &str = "lextrail-ext-dict";
/// `meta.format_version` of an extended dictionary.
pub const FORMAT_VERSION: u32 = 1;
/// `meta.format` of an audio file.
pub const AUDIO_FORMAT: &str = "lextrail-ext-audio";
/// `meta.format_version` of an audio file.
pub const AUDIO_FORMAT_VERSION: u32 = 1;

/// Tables, written before the data.
pub const SCHEMA: &str = "
    CREATE TABLE meta (
        key TEXT PRIMARY KEY,
        value TEXT NOT NULL
    ) STRICT, WITHOUT ROWID;
    CREATE TABLE entries (
        id INTEGER PRIMARY KEY,
        source_entry INTEGER NOT NULL,
        entry_key TEXT NOT NULL UNIQUE,
        headword TEXT NOT NULL,
        homograph TEXT,
        pos TEXT,
        phon_gb TEXT,
        phon_us TEXT,
        sound_gb TEXT,
        sound_us TEXT,
        cefr TEXT,
        oxford INTEGER,
        opal TEXT,
        ord INTEGER NOT NULL
    ) STRICT;
    CREATE TABLE keys (
        key TEXT NOT NULL COLLATE NOCASE,
        key_exact TEXT NOT NULL,
        entry_id INTEGER NOT NULL REFERENCES entries (id),
        relation TEXT NOT NULL,
        kinds TEXT,
        via TEXT,
        grp TEXT,
        ord INTEGER NOT NULL,
        PRIMARY KEY (key, key_exact, entry_id)
    ) STRICT, WITHOUT ROWID;
    CREATE TABLE groups (
        entry_id INTEGER NOT NULL REFERENCES entries (id),
        grp TEXT NOT NULL,
        ord INTEGER NOT NULL,
        label TEXT,
        pos TEXT,
        xrefs TEXT,
        fingerprint TEXT,
        PRIMARY KEY (entry_id, grp)
    ) STRICT, WITHOUT ROWID;
    CREATE TABLE senses (
        id INTEGER PRIMARY KEY,
        entry_id INTEGER NOT NULL REFERENCES entries (id),
        grp TEXT NOT NULL,
        sense INTEGER NOT NULL,
        cefr TEXT,
        gram TEXT,
        labels TEXT,
        hint_en TEXT,
        hint_zh TEXT,
        heading_en TEXT,
        heading_zh TEXT,
        patterns TEXT,
        def_en TEXT,
        def_zh TEXT,
        xrefs TEXT,
        fingerprint TEXT NOT NULL
    ) STRICT;
    CREATE TABLE examples (
        id INTEGER PRIMARY KEY,
        entry_id INTEGER NOT NULL REFERENCES entries (id),
        grp TEXT NOT NULL,
        sense INTEGER NOT NULL,
        ord INTEGER NOT NULL,
        pattern TEXT,
        en TEXT NOT NULL,
        zh TEXT,
        sound_gb TEXT,
        sound_us TEXT
    ) STRICT;
    CREATE TABLE forms (
        entry_id INTEGER NOT NULL REFERENCES entries (id),
        ord INTEGER NOT NULL,
        grp TEXT,
        kind TEXT,
        text TEXT NOT NULL,
        sound_gb TEXT,
        sound_us TEXT,
        PRIMARY KEY (entry_id, ord)
    ) STRICT, WITHOUT ROWID;
    CREATE TABLE variants (
        entry_id INTEGER NOT NULL REFERENCES entries (id),
        ord INTEGER NOT NULL,
        grp TEXT,
        label TEXT,
        text TEXT NOT NULL,
        PRIMARY KEY (entry_id, ord)
    ) STRICT, WITHOUT ROWID;
    CREATE TABLE notes (
        id INTEGER PRIMARY KEY,
        entry_id INTEGER NOT NULL REFERENCES entries (id),
        ord INTEGER NOT NULL,
        grp TEXT,
        kind TEXT,
        text TEXT NOT NULL
    ) STRICT;
    CREATE TABLE sense_renames (
        old_id TEXT PRIMARY KEY,
        new_id TEXT,
        how TEXT NOT NULL
    ) STRICT, WITHOUT ROWID;
";

/// Indexes, created after the data is in. `keys` is looked up by its primary key (`key`,
/// case-insensitive).
pub const INDEXES: &str = "
    CREATE INDEX senses_entry ON senses (entry_id);
    CREATE INDEX examples_entry ON examples (entry_id);
    CREATE INDEX notes_entry ON notes (entry_id);
";

/// Tables of the audio file: file name (as in the `sound://` link) → the original bytes.
pub const AUDIO_SCHEMA: &str = "
    CREATE TABLE meta (
        key TEXT PRIMARY KEY,
        value TEXT NOT NULL
    ) STRICT, WITHOUT ROWID;
    CREATE TABLE audio (
        id INTEGER PRIMARY KEY,
        name TEXT NOT NULL UNIQUE,
        data BLOB NOT NULL
    ) STRICT;
";

/// Relations between a lookup key and an entry, in the order they are tried
/// (`docs/specs/m1.5-oxford.md` 6.1).
pub const RELATIONS: [&str; 7] = [
    "headword",
    "inflection",
    "variant",
    "derivative",
    "idiom",
    "phrasal",
    "other",
];
