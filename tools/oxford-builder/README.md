# tools/oxford-builder

Converts the Oxford Advanced Learner's English-Chinese Dictionary, 10th edition (MDX + MDD, supplied by the user, never in the repository) to a LexTrail extended dictionary (D049). Rules and formats: `docs/specs/m1.5-oxford.md` sections 3–6 and 9.

```bash
cargo run -p lextrail-oxford-builder --release -- \
  --mdx <.mdx> --mdd <.mdd> --mdd <.1.mdd> \
  --ecdict data/build/lextrail-dict-core.db --out <folder> \
  [--previous <old oalecd10.db>] [--report <report.json>]
```

- Writes `oalecd10.db` (text) and `oalecd10-audio.db` (headword, form and idiom pronunciations, original bytes) into `--out`. Both files are written to temporary files in the same folder and replace the old ones only when both are finished, so a failed or interrupted run (Ctrl+C) leaves both old files as they were. If LexTrail has a file open, the converter stops before writing anything.
- `--ecdict` is required: the relation rules use its word forms. Rebuild it first (`cargo run -p lextrail-dict-builder --release`).
- `--previous` maps sense and group ids of an earlier conversion to the new ones (`sense_renames`: `same_entry`, `cross_entry`, `changed` — same id, content changed — or `removed`).
- The same source, converter version and ECDICT file give the same content (only `meta.built_at` differs).
- The report (terminal, and JSON with `--report`) has counts only: entries, keys by relation, senses, coverage, unknown classes, audio.
- Shared rules (ids, fingerprints, key folding) live in `crates/lextrail-dict` (`ext`).

Development aids (print structure or counts, or write entry HTML to a folder you choose, outside the repository; `dump` refuses a folder inside it):

```bash
cargo run -p lextrail-oxford-builder --release --example classes -- <.mdx>
cargo run -p lextrail-oxford-builder --release --example probe -- <.mdx> [<.mdd>...]
cargo run -p lextrail-oxford-builder --example dump -- <.mdx> <out folder> <key>... | grep:<text>
cargo run -p lextrail-oxford-builder --release --example other_keys -- <oalecd10.db> <ECDICT .db>
```

Tests use made-up entries only (`tests/fixtures/`).
