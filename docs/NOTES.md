# 开发笔记

> 长期有效的"怎么做"：代码现状、常用命令、debug 开关、测试方法与坑。当前进度见 `STATUS.md`，决定见 `DECISIONS.md`。
> 新的约定和测试方法写在这里（`AGENTS.md` 结束流程）。

---

## 代码现状

各 crate 和模块的职责（细节见各文件）：

- `crates/lextrail-core`：HLC（`Hlc`、`HlcClock`、可注入的 `Clock`）；`lookup`（`LookupKind`、`CaptureMethod`、`normalize_text`、30 秒去重）；`text`：`sentence_around` 切句（处理缩写、小数、省略号、引号、中文标点、硬换行、开头的列表符号），`clean_control_chars` 过滤控制字符、PDFium 的行末连字符 `\u{2}` 还原为 `-`，`resolve_line_end_hyphens`（词典里查得到就合并，如 compu-tation），另有 `join_unmarked_line_breaks`、`starts_sentence`。
- `crates/lextrail-capture`（D041、D043）：`TextCapture`（`snapshot` / `capture` / `capture_traced` / `capture_context`，最后一个目前返回 `None`，做连按两次 Ctrl+C 时实现）；`Pipeline` 按顺序运行各步骤，步骤间用 `StepContext` 传递信息，UIA 的"没有选中"默认就是最终结果；`clipboard_rules.rs`（剪贴板兜底的规则，与平台无关）；`windows/`：`window.rs`（快照）、`uia.rs`（UIA 步骤）、`clipboard.rs`（剪贴板步骤）。`capture_traced` 接收进度回调（`CaptureProgress`：等松开快捷键 / 已松开），步骤结果带 `key_wait`。`examples/capture_probe.rs`（手动测试真实窗口）、`examples/uia_dump.rs`（打印原始 UIA 数据）。测试 `clipboard_round_trip` 默认忽略（会改动真实剪贴板）。
- `crates/lextrail-store`（D037、D040）：SQLite（在 `%LOCALAPPDATA%`，按需打开）、`user_version` 迁移、`meta` / `lookups` 表、`record_lookup` 等。桌面端尚未接入。
- `crates/lextrail-config`（D035）：设置文件与凭据管理器。
- `crates/lextrail-dict`（D044–D046）：查询、词形还原、输入分类、`is_word`（合并断词用，含完整版词头表 `extra_words`）、`view`（显示用的结果）。`examples/view.rs` 打印某个词的显示结果（JSON）。
- `apps/desktop/src-tauri`：托盘、单实例、主窗口（占位）、设置、全局快捷键（`hotkey.rs` + `keyboard.rs` 屏蔽键与防卡键钩子）、`dictionary.rs`（词典常驻服务，第一次取到文字时打开，`cache_size` 1 MiB、不用 mmap）、悬浮窗 `popup/`：
  - `lifecycle.rs` 窗口生命周期，`request.rs` 一次查词，`activation.rs` 获取焦点，`foreground.rs` 前台窗口监视，`content.rs` 内容事件（合并断词、分类、查词），`placement.rs` 位置，`timing.rs` bench 计时，`webview.rs` 挂起 / 恢复。
  - 悬浮窗页面先订阅内容事件再调用 `popup_ready`，之后才显示；权限在 `capabilities/popup.json`（只有 `core:event:allow-listen`）。
- `apps/desktop/src`：悬浮窗页面（`popup/`：`content.ts`、`ipc.ts`、`App.vue`、`DictResult.vue`），主窗口占位。**前端还没有测试框架**。
- `tools/dict-builder`：ECDICT → 精简版 SQLite（D044，格式版本 2，文件约 35.7 MiB）。
- `tools/bench`：性能测试；应用在 `LEXTRAIL_BENCH_TIMINGS` 下写出分项计时；`--lookup-text TEXT` 在词典打开的状态下测（m0-benchmark 7.3）。
- `tools/e2e/`：`popup-e2e.ps1`（悬浮窗端到端检查，见下文）、`stuck-keys.ps1`（屏蔽键防卡键压力测试）、`list-item.html`、`key-log.html`（记录页面收到的键盘事件）。

已实现的行为要点（设计理由见对应决定）：

- **取词**（D041）：UIA 在专用 MTA 线程上运行；Chromium 窗口第一次取词时在预算内重试；Edge 的 PDF 阅读器报告的选区会错位，所以不走 UIA（`[chromium_pdf]`），Chrome 的 PDF 正常走 UIA。
- **悬浮窗焦点方案 B**（D042）：取词期间不抢焦点，超过约 100 ms 才显示"正在取词…"（按住快捷键的时间不算，松开时重新计时；按住超过约 1 秒显示"松开快捷键后取词…"），取词完成后获取焦点（`SetForegroundWindow`，被拒时用 `AttachThreadInput`）；仍失败时不带焦点显示结果，可用右上角 × 关闭；悬浮窗在前台时再按快捷键会被忽略；每次显示时按目标显示器当前缩放设置尺寸。
- **快捷键屏蔽键**（D014 补充）：快捷键触发时 Alt 仍按着，就对仍按着的 Ctrl（其次 Shift）补发一次按下、不发松开；`0xE8` 只在只有 Alt 时使用（它会让 Windows Terminal 清掉选区，R3-P1）。补发期间在单独线程上装几毫秒的低级键盘钩子，用户恰好在补发前松开时再补发松开。执行顺序：屏蔽键 → 记录前台窗口 → 启动取词线程 → 请求显示悬浮窗（第一次时创建窗口）。
- **剪贴板兜底**（D043）：等松开快捷键再复制，最多等 3 秒，超过时提示"请松开快捷键后再试"；UIA 不支持、超时或出错时运行；UIA 说"没有选中"时只对 Chromium 系窗口用剪贴板确认。终端（Windows Terminal、conhost、VS Code 集成终端）用 Ctrl+Insert；VS Code 的空选区复制（整行）识别为"没有选中"；复制前保存剪贴板、复制后写回，写回的内容不进剪贴板历史。
- **行末断词**（D044 补充，`m1-popup-dict.md` 第 6 节）：Chrome PDF（UIA）的 PDFium 标记 U+0002 参与合并；Edge PDF（剪贴板）连字符被丢掉，用猜测规则（左半段不是词、合起来是词才合并）。选中的文字和句子都合并。
- 选中的内容和整个上下文相同时，悬浮窗不显示句子行（有意的设计）。

## 常用命令

- `cargo run -p lextrail-dict-builder --release`：生成词典 `data/build/lextrail-dict-core.db`。**编译桌面程序之前必须先生成**（它是 Tauri 资源，缺失时 `tauri-build` 报错）
- `cargo run -p lextrail-dict --release --example view -- data/build/lextrail-dict-core.db better self-attention`：看某个词在悬浮窗里会显示什么
- `pnpm check`：格式、lint、类型检查、全部测试（含 release 配置的 clippy）
- `pnpm --filter @lextrail/desktop tauri build --debug --no-bundle`：带日志的 debug 版（`target\debug\lextrail-desktop.exe`，在终端里运行看日志）
- `pnpm --filter @lextrail/desktop tauri build --no-bundle`：真正的 release 版。**不要用** `cargo build --release`（会加载开发服务器页面）
- `powershell -NoProfile -ExecutionPolicy Bypass -File tools/e2e/popup-e2e.ps1 -App target/debug/lextrail-desktop.exe [-Pdf] [-HoldMs 2000]`：悬浮窗端到端检查（会移动鼠标、模拟按键）
- `powershell -NoProfile -ExecutionPolicy Bypass -Command "& ./tools/e2e/stuck-keys.ps1 -App target/debug/lextrail-desktop.exe -GapsMs 0,0,1,2,3 -Rounds 24"`：屏蔽键防卡键压力测试（模拟按键；数组参数要用 `-Command` 传）
- `cargo run -p lextrail-capture --example capture_probe -- 3`：3 秒后对前台窗口取词并打印结果
- `cargo test -p lextrail-capture -- --ignored clipboard_round_trip`：会改动真实剪贴板的测试
- `cargo run -p lextrail-bench --release -- quick`：性能测试（先 `tauri build`；运行时不要操作电脑）
- `cargo audit`：依赖漏洞检查

## debug 开关与日志

只在 debug 构建中有效，用法见 `m1-capture-manual-tests.md` 第 0 节：

- `LEXTRAIL_DEBUG_CAPTURE_DELAY_MS`：让取词变慢
- `LEXTRAIL_DEBUG_SKIP_ACTIVATION`：不获取焦点
- `LEXTRAIL_DEBUG_NO_ALT_MASK`：关掉屏蔽键
- `LEXTRAIL_DEBUG_ALT_MASK_E8`：屏蔽键总用旧的 `0xE8`，用来对比
- `LEXTRAIL_DEBUG_CLIPBOARD_ONLY`：跳过 UIA，只走剪贴板
- `LEXTRAIL_DEBUG_RESUME_DELAY_MS`：让 WebView 的 `Resume` 变慢（复现 R4-P1 一类问题）
- `LEXTRAIL_DEBUG_LOG_CONTENT=1`：`request: ...` 日志打印完整内容（取到的文字、句子、词典结果）；默认只打印种类，如 `SetContent(Selection{..})`、`Unavailable(no_selection)`（D032）

debug 版日志：

- 每次取词一行，如 `capture #2: captured | start +7.6ms; uia=no_selection 16.8ms (uia_focus); clipboard=captured 160.3ms keys=150.1ms [ctrl_insert]; total 177.1ms`（不含取到的文字；`start` 是从按下快捷键到开始取词，`keys` 是等松开快捷键的时间）。
- 每次补发屏蔽键打印 `hotkey: alt mask: ...`。
- `request: ...`（悬浮窗显示与焦点）、`popup: ...`（窗口生命周期）。

## 编码规范细节

- 提交说明示例：`feat(capture): add UIA selection reader`、`fix(popup): resume webview before show`、`docs(status): update after M1 capture work`。
- Rust：`cargo fmt`；`cargo clippy -- -D warnings` 无警告。`pnpm check` 含 release 配置的 clippy（CI 的 release 构建用 `-D warnings`，只在 debug 版使用的名字在 release 下会报"未使用"，用 `#[cfg(any(debug_assertions, test))]` 等处理）。
- 前端：TypeScript strict；ESLint + Prettier。
- 测试：Rust 每个 crate 有单元测试；前端关键组件和流式解析有测试；CI 全部通过才可合并。

## 性能测试（bench）

- 口径见 D036，结果追加到 `docs/specs/m0-benchmark.md` 第 7 节"历次重测"。
- **要在普通终端窗口（如 Windows Terminal）中运行**：从 Claude 桌面应用等 Chromium 系窗口里启动时，前台是它，每次多等约 400 ms 的剪贴板确认，结果不可比。
- 看结果时先看"capture (in app)"是否正常（约 20–35 ms）；约 430 ms 说明 bench 终端没有一直在前台。
- 运行时不要操作电脑；AI 运行时放到后台，只取摘要行。

## 测试与验证

- **端到端验证必须看到悬浮窗**：用 `popup-e2e.ps1`（UI Automation 读出悬浮窗文字核对）或截图亲眼看到内容；只看日志不算。能用 UIA 读文字核对就不截图；布局、滚动等必须看画面时才截图，只截悬浮窗区域并缩小。
- 用模拟输入做调查或测试之前和之后都要告诉项目负责人（项目负责人的要求）。
- 手动测试清单与历次结果：`docs/specs/m1-capture-manual-tests.md`（只读当前轮次那一节）。
- 调试卡死 / 崩溃的 dump：用 WinDbg 的 cdb；debug 版不可逐字节复现，分析旧提交的 dump 要在同一路径下重新构建拿 pdb。

模拟输入测试的坑（本机）：

- 中文输入法会转换模拟键入的文字，改用粘贴。
- Ctrl+` 是本机另一个截图 / 贴图工具的全局快捷键。
- 新配置的 Edge 会弹出同步登录对话框（用 `--inprivate`）和"翻译此页"提示（先按 Esc）。
- `WScript.Shell.AppActivate("标题")` 可能激活资源管理器的 `TabProxyWindow`，改按进程 ID 激活；嵌套的 `powershell -File` 子进程可能拿不到前台，鼠标键盘辅助脚本直接在当前 PowerShell 里运行，激活后检查 `GetForegroundWindow`。
- 页面 JS 在 Chromium 开启无障碍之前做的选区不会被报告，用真实鼠标双击；在已选中的词里开始拖动会变成拖放，先点别处。
- 用 `Start-Process -WindowStyle Hidden` 启动时，第一个提示框会被系统隐藏（正常启动不受影响）。

## 已知情况（不需要处理）

- GitHub Dependabot 有 1 个中等级别提醒（`glib 0.18.5`，RUSTSEC-2024-0429），只经由 Tauri 的 Linux GTK 依赖引入，Windows 构建里没有这个 crate。`cargo audit` 另有一条 `proc-macro-error` unmaintained 警告，同样只在 Linux 上用到。
- TypeScript 锁定在 `~6.0.3`（`typescript-eslint` 8.x 不支持 7.x）。
- 开发环境是 Windows；性能测试和手动测试必须在真实的 Windows 机器上进行。仓库公开，不要提交密钥、Token 或个人数据。
