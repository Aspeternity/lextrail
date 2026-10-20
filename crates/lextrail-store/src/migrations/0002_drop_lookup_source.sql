-- Where a word came from (program, web address) is no longer shown or stored (D046
-- supplement). Needs SQLite 3.35+ (bundled: 3.53); neither column is indexed or checked.
ALTER TABLE lookups DROP COLUMN source_app;
ALTER TABLE lookups DROP COLUMN url;
