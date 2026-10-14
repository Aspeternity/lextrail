# apps/desktop

Tauri 2 desktop app. `src-tauri/`: tray, hotkeys, window management, calls into `crates/`. `src/`: Vue frontend — `popup/` (lightweight standalone entry) and `main/` (main window).

Run in development (from the repository root):

```bash
pnpm --filter @lextrail/desktop tauri dev
```

The app starts with no window — only a tray icon — so no WebView is loaded until a window is opened.
