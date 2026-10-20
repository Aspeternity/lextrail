# tools/dict-builder

Builds the compact dictionary (`lextrail-dict-core.db`) from a pinned ECDICT commit (D038, D044). Format, selection and sense-splitting rules: `docs/specs/m1-dict.md`.

```bash
cargo run -p lextrail-dict-builder --release              # download (if needed) + build
cargo run -p lextrail-dict-builder --release -- --offline # use the already downloaded source
```

- The source is downloaded to `data/raw/` and checked against the pinned size and SHA-256 (`src/source.rs`); a file that fails is deleted. The dictionary is written to `data/build/`. Both directories are git-ignored.
- The output is deterministic: the same source and builder version give a byte-identical file.
- Sense splitting, `exchange` parsing and phonetic normalization live in `crates/lextrail-dict`, shared with the reader.
