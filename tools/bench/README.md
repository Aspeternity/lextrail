# tools/bench

M0 performance benchmark (`lextrail-bench`) for the budgets in `docs/ARCHITECTURE.md` section 10. Results and method: `docs/specs/m0-benchmark.md`.

```bash
pnpm --filter @lextrail/desktop tauri build --no-bundle
cargo run -p lextrail-bench --release -- quick
cargo run -p lextrail-bench --release -- long --wait-mins 120
```

Windows only. It stops any running LexTrail, launches the release build, and drives it with simulated Alt+Space (the default hotkey) / Esc presses while moving the mouse cursor, so leave the machine alone while it runs (`long` only touches input at the start and end). It refuses to run unless the settings file uses the default hotkey and `responsive` popup mode.

Note: a plain `cargo build --release` of the desktop app runs Tauri in dev mode (it loads `devUrl`); always benchmark a `tauri build` binary.
