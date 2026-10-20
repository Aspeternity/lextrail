# 开发手册

> 常用命令、debug 开关、测试工具与注意事项。**不必每次开始时读**，编译、运行、测试时再查对应一节。
> 进度看 `STATUS.md`，架构看 `ARCHITECTURE.md`。

---

## 1. 常用命令

- `cargo run -p lextrail-dict-builder --release`：生成词典 `data/build/lextrail-dict-core.db`（格式版本 2）。**编译桌面程序之前必须先生成**（它是 Tauri 资源，缺失时 `tauri-build` 报错）
- `cargo run -p lextrail-dict --release --example view -- data/build/lextrail-dict-core.db better self-attention`：看某个词在悬浮窗里会显示什么
- `pnpm check`：格式、lint、类型检查、全部测试（含 release 配置的 clippy）
- `pnpm --filter @lextrail/desktop tauri build --debug --no-bundle`：带日志的 debug 版（`target\debug\lextrail-desktop.exe`，在终端里运行看日志）
- `pnpm --filter @lextrail/desktop tauri build --no-bundle`：真正的 release 版。**不要用** `cargo build --release`（会加载开发服务器页面）
- `powershell -NoProfile -ExecutionPolicy Bypass -File tools/e2e/popup-e2e.ps1 -App target/debug/lextrail-desktop.exe [-Pdf] [-HoldMs 2000]`：悬浮窗端到端检查（会移动鼠标、模拟按键）
- `powershell -NoProfile -ExecutionPolicy Bypass -Command "& ./tools/e2e/stuck-keys.ps1 -App target/debug/lextrail-desktop.exe -GapsMs 0,0,1,2,3 -Rounds 24"`：屏蔽键防卡键压力测试（模拟按键；数组参数要用 `-Command` 传）
- `cargo run -p lextrail-capture --example capture_probe -- 3`：3 秒后对前台窗口取词并打印结果；`--example uia_dump` 打印原始 UIA 数据
- `cargo test -p lextrail-capture -- --ignored <名字>`：运行默认忽略的 `clipboard_round_trip`、`listener_round_trip`（会改动真实剪贴板）
- `cargo run -p lextrail-bench --release -- quick`：性能测试（先 `tauri build`；运行时不要操作电脑；`--lookup-text TEXT` 在词典打开的状态下测）
- `cargo audit`：依赖漏洞检查

## 2. 代码地图

- `crates/lextrail-core`：HLC；`lookup`（`LookupKind`、`CaptureMethod`、`normalize_text`、30 秒去重）；`text`（切句、控制字符过滤、行末连字符）
- `crates/lextrail-capture`（D041、D043）：`TextCapture`、`Pipeline`；`clipboard_rules.rs`、`double_copy.rs`（与平台无关）；`windows/`：`window.rs`、`uia.rs`、`clipboard.rs`、`double_copy.rs`（`DoubleCopyListener`）
- `crates/lextrail-dict`（D044–D046）：查询、词形还原、输入分类、`is_word`、`view`
- `crates/lextrail-store`（D037、D040）：SQLite、`user_version` 迁移、`meta` / `lookups`
- `crates/lextrail-config`（D035）：设置文件与凭据管理器
- `apps/desktop/src-tauri`：托盘、单实例、主窗口（占位）、`hotkey.rs` + `keyboard.rs`（屏蔽键与防卡键钩子）、`dictionary.rs`、`history.rs`、`popup/`（`lifecycle`、`request`、`activation`、`foreground`、`content`、`placement`、`sizing`、`timing`）。悬浮窗权限在 `capabilities/popup.json`
- `apps/desktop/src`：悬浮窗页面（`popup/`），主窗口占位。**前端还没有测试框架**
- `tools/e2e/`：`popup-e2e.ps1`、`stuck-keys.ps1`、`key-observer.ps1`、测试页面；`tools/bench`：性能测试

## 3. debug 版开关

只在 debug 构建中有效，用法见 `specs/m1-capture-manual-tests.md` 第 0 节：

- `LEXTRAIL_DEBUG_CAPTURE_DELAY_MS`：让取词变慢
- `LEXTRAIL_DEBUG_SKIP_ACTIVATION`：不获取焦点
- `LEXTRAIL_DEBUG_FORCE_ATTACH`：强制走获取焦点的后备路径（`popup-e2e.ps1 -ForceAttach`）
- `LEXTRAIL_DEBUG_NO_ALT_MASK`：关掉屏蔽键；`LEXTRAIL_DEBUG_ALT_MASK_E8`：屏蔽键总用旧的 `0xE8`（对比用）
- `LEXTRAIL_DEBUG_CLIPBOARD_ONLY`：跳过 UIA，只走剪贴板
- `LEXTRAIL_DEBUG_CONFIG_DIR` / `LEXTRAIL_DEBUG_DATA_DIR`：设置文件、数据库改用指定目录（`popup-e2e.ps1`、`stuck-keys.ps1` 已默认用临时目录）

debug 版日志：每次取词打印一行，如 `capture #2: captured | start +7.6ms; uia=no_selection 16.8ms (uia_focus); clipboard=captured 160.3ms keys=150.1ms [ctrl_insert]; total 177.1ms`（不含取到的文字；`start` 是按下快捷键到开始取词，`keys` 是等松开快捷键的时间），另有 `history recorded|repeat|skipped`、`hotkey: alt mask: ...`、带时间戳的 `popup:` / `request:` 行。

## 4. 测试规则（项目负责人的要求）

- **自动测试一律用临时目录**，不碰项目负责人的真实设置和数据库。
- 用模拟输入做调查或测试**之前和之后都要告诉项目负责人**。
- 模拟按键之前先确认前台窗口是测试要的那个，不是就不发；截图只截悬浮窗、提示框或测试窗口自己的区域，不截全屏。
- 端到端验证必须截图看到悬浮窗内容或用 `popup-e2e.ps1`，只看日志不算。
- 需要日志的真实键盘测试：由 AI 启动程序、读日志，项目负责人只按键、报现象。
- `key-observer.ps1` 的日志里有机器上的所有按键，用完即删、不提交。

## 5. 已知的坑

- **bench 要在普通终端窗口（如 Windows Terminal）中运行**：从 Claude 桌面应用等 Chromium 系窗口里启动时，前台是它，每次多等约 400 ms 的剪贴板确认，结果不可比。看结果时先看"capture (in app)"是否正常（约 20–35 ms）。
- 用 `Start-Process -WindowStyle Hidden` 启动时，第一个提示框会被系统隐藏。正常启动不受影响。
- 本机模拟输入：中文输入法会转换模拟键入的文字（改用粘贴）；Ctrl+` 是本机另一个截图工具的全局快捷键；新配置的 Edge 会弹出同步登录对话框（用 `--inprivate`）和"翻译此页"提示（先按 Esc）。
- TypeScript 锁定在 `~6.0.3`（`typescript-eslint` 8.x 不支持 7.x）。
- 依赖提醒：`glib 0.18.5`（RUSTSEC-2024-0429）和 `proc-macro-error`（unmaintained）只经由 Tauri 的 Linux 依赖引入，Windows 构建没有；等 Tauri 升级到 glib 0.20 以上再看。
