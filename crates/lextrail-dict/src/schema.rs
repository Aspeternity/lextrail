//! The dictionary file's tables (`docs/specs/m1-dict.md` section 5), shared by the builder
//! (writer) and [`crate::Dictionary`] (reader).

/// Tables. Written by `tools/dict-builder` before the data.
pub const SCHEMA: &str = "
    CREATE TABLE meta (
        key TEXT PRIMARY KEY,
        value TEXT NOT NULL
    ) STRICT, WITHOUT ROWID;
    CREATE TABLE entries (
        id INTEGER PRIMARY KEY,
        word TEXT NOT NULL,
        phonetic TEXT,
        collins INTEGER,
        oxford INTEGER,
        bnc INTEGER,
        frq INTEGER,
        tags TEXT
    ) STRICT;
    CREATE TABLE senses (
        entry_id INTEGER NOT NULL REFERENCES entries (id),
        ord INTEGER NOT NULL,
        pos TEXT,
        domain TEXT,
        text TEXT NOT NULL,
        PRIMARY KEY (entry_id, ord)
    ) STRICT, WITHOUT ROWID;
    CREATE TABLE forms (
        form TEXT NOT NULL,
        lemma TEXT NOT NULL,
        kind TEXT NOT NULL,
        PRIMARY KEY (form, lemma, kind)
    ) STRICT, WITHOUT ROWID;
    CREATE TABLE extra_words (
        word TEXT PRIMARY KEY
    ) STRICT, WITHOUT ROWID;
";

/// Indexes, created after the data is in (faster, and a compact index).
pub const INDEXES: &str = "
    CREATE INDEX entries_word ON entries (word COLLATE NOCASE);
    CREATE INDEX forms_form ON forms (form COLLATE NOCASE);
";
