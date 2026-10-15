-- Local key/value data for this device: device_id, last issued HLC. Never synced.
CREATE TABLE meta (
  key   TEXT PRIMARY KEY,
  value TEXT NOT NULL
) STRICT;

-- Lookup history (ARCHITECTURE.md 6.3). Insert and soft delete only.
CREATE TABLE lookups (
  id          TEXT    PRIMARY KEY,
  created_at  INTEGER NOT NULL,
  updated_at  TEXT    NOT NULL,
  device_id   TEXT    NOT NULL,
  deleted     INTEGER NOT NULL DEFAULT 0 CHECK (deleted IN (0, 1)),
  query       TEXT    NOT NULL,
  kind        TEXT    NOT NULL CHECK (kind IN ('word', 'phrase', 'sentence')),
  lemma       TEXT,
  context     TEXT,
  source_app  TEXT,
  url         TEXT,
  method      TEXT    NOT NULL CHECK (method IN ('uia', 'clipboard', 'clipboard_double_copy', 'ocr'))
) STRICT;

CREATE INDEX lookups_lemma_created ON lookups (lemma, created_at);
CREATE INDEX lookups_created ON lookups (created_at);
