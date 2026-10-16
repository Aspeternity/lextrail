# 项目进度交接单

> 每次开发结束前更新本文件（见 `AGENTS.md` 的"结束流程"）。
> 只保留**当前**状态；已完成的历史以 `ROADMAP.md` 的勾选和 `git log` 为准。

**最后更新**：2026-10-17
**当前阶段**：M1 · 取词与词典（进行中）。取词部分（UIA + 剪贴板兜底 + 悬浮窗焦点）已完成，**等第三轮手动复测（R3）**；词典部分尚未开始。

---

## 当前状态

M0 全部完成（路线 A 已确认，D036）。M1 已完成：

- **数据模型公共部分**评审（D039）：同步元数据、HLC 时间戳格式、`lookups` 表，见 `ARCHITECTURE.md` 6.1–6.3，推迟事项在 6.5。
- **`lextrail-core` 的 HLC** 与 **`lextrail-store`**（D037、D040：数据库在 `%LOCALAPPDATA%`，按需打开）。**桌面端尚未接入查询历史。**
- **取词**（D041）：统一接口 + Windows UIA，在专用 MTA 线程上运行；Chromium 窗口第一次取词时在预算内重试；Edge 的 PDF 阅读器报告的选区会错位，所以不走 UIA（`[chromium_pdf]`），Chrome 的 PDF 正常走 UIA。
- **悬浮窗焦点方案 B**（D042）：取词期间不抢焦点，超过约 100 ms 才显示"正在取词…"，取词完成后获取焦点（`SetForegroundWindow`，被拒时用 `AttachThreadInput`；不再用 Tauri 模拟 Alt 的后备做法）；仍失败时不带焦点显示结果，可用右上角 × 关闭；悬浮窗在前台时再按快捷键会被忽略；每次显示时按目标显示器当前缩放设置尺寸。
- **快捷键屏蔽键**（D014 补充）：快捷键触发时 Alt 仍按着，就补发虚拟键 `0xE8`，避免 VS Code 等程序把它当成单独按 Alt 而激活菜单栏（R2-1 用真实键盘验证有效）。
- **剪贴板兜底**（D043）：UIA 不支持、超时或出错时运行；UIA 说"没有选中"时只对 Chromium 系窗口用剪贴板确认。终端（Windows Terminal、conhost、VS Code 集成终端）用 Ctrl+Insert；VS Code 的空选区复制（整行）识别为"没有选中"；复制前保存剪贴板、复制后写回，写回的内容不进剪贴板历史。已在记事本、Edge PDF、conhost、Windows Terminal、VS Code 编辑器和终端中实测（截图核对）。
- **文字处理**（`lextrail-core::text`）：`sentence_around` 切句（处理缩写、小数、省略号、引号、中文标点、硬换行、开头的列表符号）；`clean_control_chars` 过滤控制字符，PDFium 的行末连字符 `\u{2}` 还原为 `-`；`resolve_line_end_hyphens`（词典里查得到就合并，如 compu-tation）已写好，**等词典接入时调用**。

手动测试：清单和全部结果在 `docs/specs/m1-capture-manual-tests.md`。
- 第一轮（R1，2026-10-16）：发现 11 个问题（R1-1 至 R1-11），已分两阶段处理。
- 第二轮（R2，2026-10-16）：第一阶段修复后复测，12 项全部通过，第一阶段验收。
- **第三轮（R3）：剪贴板兜底完成后的复测清单（R3-1 至 R3-14）已写好，等项目负责人测。**

代码现状（详见各文件）：
- `crates/lextrail-core`：HLC（`Hlc`、`HlcClock`、可注入的 `Clock`）；`lookup`（`LookupKind`、`CaptureMethod`、`normalize_text`、30 秒去重）；`text`（见上）。46 个测试
- `crates/lextrail-capture`（D041、D043）：`TextCapture`（`snapshot` / `capture` / `capture_traced` / `capture_context`，最后一个目前返回 `None`，做连按两次 Ctrl+C 时实现）；`Pipeline` 按顺序运行各步骤，步骤间用 `StepContext` 传递信息，UIA 的"没有选中"默认就是最终结果；`clipboard_rules.rs`（剪贴板兜底的规则，与平台无关）；`windows/`：`window.rs`（快照）、`uia.rs`（UIA 步骤）、`clipboard.rs`（剪贴板步骤）。30 个测试，其中 `clipboard_round_trip` 默认忽略（会改动真实剪贴板，需要时用 `cargo test -p lextrail-capture -- --ignored clipboard_round_trip`）。`examples/capture_probe.rs`（手动测试真实窗口）、`examples/uia_dump.rs`（打印原始 UIA 数据）
- `crates/lextrail-store`（D037、D040）：SQLite、`user_version` 迁移、`meta` / `lookups` 表、`record_lookup` 等。20 个测试
- `crates/lextrail-config`（D035）：设置文件与凭据管理器。17 个测试
- `apps/desktop/src-tauri`：托盘、单实例、主窗口（占位）、设置、全局快捷键（`hotkey.rs` + `keyboard.rs` 屏蔽键）、悬浮窗（`popup/`：`lifecycle.rs` 窗口生命周期 20 个测试，`request.rs` 一次查词 9 个测试，`activation.rs` 获取焦点，`foreground.rs` 前台窗口监视，`content.rs` 内容事件，`placement.rs` 位置，`timing.rs` bench 计时）。悬浮窗页面先订阅内容事件再调用 `popup_ready`，之后才显示；权限在 `capabilities/popup.json`（只有 `core:event:allow-listen`）。共 56 个测试
- `apps/desktop/src`：悬浮窗页面（`popup/`：`content.ts`、`ipc.ts`、`App.vue`），主窗口占位。**前端还没有测试框架**
- `tools/e2e/`：`popup-e2e.ps1`（悬浮窗内容端到端检查：真实鼠标、真实快捷键、全新 Edge，用 UI Automation 读出悬浮窗文字核对并截图；`-Pdf` 走剪贴板路径）、`list-item.html`、`key-log.html`（记录页面收到的键盘事件）
- `tools/bench`：性能测试；应用在 `LEXTRAIL_BENCH_TIMINGS` 下写出分项计时。14 个测试

debug 版开关（只在 debug 构建中有效，用法见手动测试清单第 0 节）：`LEXTRAIL_DEBUG_CAPTURE_DELAY_MS`（让取词变慢）、`LEXTRAIL_DEBUG_SKIP_ACTIVATION`（不获取焦点）、`LEXTRAIL_DEBUG_NO_ALT_MASK`（关掉屏蔽键）、`LEXTRAIL_DEBUG_CLIPBOARD_ONLY`（跳过 UIA，只走剪贴板）。debug 版每次取词在控制台打印一行，如 `capture #2: captured | uia=no_selection 16.8ms (uia_focus); clipboard=captured 11.3ms [ctrl_insert]; total 28.1ms`（不含取到的文字）。

性能（`docs/specs/m0-benchmark.md` 第 7 节，口径见 D036）：最近一次（2026-10-16）托盘 1.6 MiB、挂起 83.1 MiB、冷启动 281 ms、挂起后恢复 18.5 ms，全部达标。剪贴板兜底之后还没有重测（它只在 UIA 取不到时运行，不影响 bench 的路径）。

常用命令：
- `pnpm check`：格式、lint、类型检查、全部测试
- `pnpm --filter @lextrail/desktop tauri build --debug --no-bundle`：带日志的 debug 版（`target\debug\lextrail-desktop.exe`，在终端里运行看日志）
- `pnpm --filter @lextrail/desktop tauri build --no-bundle`：真正的 release 版。**不要用** `cargo build --release`（会加载开发服务器页面）
- `powershell -NoProfile -ExecutionPolicy Bypass -File tools/e2e/popup-e2e.ps1 -App target/debug/lextrail-desktop.exe [-Pdf]`：悬浮窗端到端检查（会移动鼠标、模拟按键）
- `cargo run -p lextrail-capture --example capture_probe -- 3`：3 秒后对前台窗口取词并打印结果
- `cargo run -p lextrail-bench --release -- quick`：性能测试（先 `tauri build`；运行时不要操作电脑）
- `cargo audit`：依赖漏洞检查

## 正在进行

- 无（第二阶段已完成，等 R3 复测结果）。

## 下一步

1. **项目负责人做第三轮复测（R3-1 至 R3-14）**，结果填进手动测试清单的 R3 表并发回；先处理复测发现的问题。R3-11 至 R3-13（剪贴板历史、图片、文件的恢复）只能人工检查。
2. 然后按 `ROADMAP.md` 继续 M1：连按两次 Ctrl+C（实现 `capture_context`，完成后把这个用法加回提示文字，R1-9）→ `tools/dict-builder`（D038）→ `lextrail-dict` → 悬浮窗显示词典结果 → 行末连字符接入词典（用 `transduc-tion` 验证）→ 查询历史接入桌面端 → 快捷键设置界面。
3. 桌面端接入查询历史时注意（项目负责人的要求）：第一次查询时用 `app_local_data_dir()` 按需打开 `Store`；`Store` 放进 `Mutex`，在后台线程读写；打开失败时用 `notice::warn` 提示，之后不记录历史、照常查词；加上"不记录查询历史"开关。
4. 悬浮窗加入词典结果后重测性能预算（M1 任务）。

待跟进（已在 ROADMAP 中，或在此记录）：
- **B3 没有结论**：VS Code 编辑器里 UIA 报告"没有选中"的原因未确认。把 `editor.accessibilitySupport` 设为 `on` 后，我用模拟按键没能做出选区（屏幕阅读器模式下 Ctrl+Shift+→ 的行为不同），这次测试不成立。剪贴板兜底已覆盖 VS Code，且不改用户设置，所以暂不继续；如需要，可在 `on` 模式下手动选词，用 `capture_probe` 看 UIA 是否取到。
- **获取焦点的后备路径未在真实环境中触发过**（R2-5）：`AttachThreadInput` 重试只经代码审查；"被拒后不带焦点显示"只用模拟开关验证过。以后如在日志里看到 `Attached` 或 `Refused`，记下当时的场景。
- 前端测试框架（Vitest）还没引入（M1 任务）。
- 低优先级：Windows Terminal 跨行选中时句子带上提示符（R1-10）；句首省略号、过长上下文截取（R1-11）。
- 低优先级：冷启动后 0–100 ms 内按 Esc 可能无效（`m0-benchmark.md` 4.5 节）。
- 用模拟输入做调查或测试之前和之后都要告诉项目负责人（项目负责人的要求）；端到端验证必须截图看到悬浮窗内容或用 `popup-e2e.ps1`，只看日志不算。

## 遇到的问题 / 需要项目负责人决定

- 无需要现在决定的事项。服务端许可证写法和 `apps/server` 是否加入 Cargo workspace 推迟到 M4（见 `DECISIONS.md` 待确认事项）。
- 需要关注：冷启动两次重测分别为 351 ms 和 281 ms（预算 400 ms），波动较大；冷启动几乎全部是窗口创建和页面加载（约 274 ms），取词与之并行。接入词典后要再测，看趋势时多测几次。

已知情况（不需要处理）：
- GitHub Dependabot 有 1 个中等级别提醒（`glib 0.18.5`，RUSTSEC-2024-0429），只经由 Tauri 的 Linux GTK 依赖引入，Windows 构建里没有这个 crate。`cargo audit` 另有一条 `proc-macro-error` unmaintained 警告，同样只在 Linux 上用到。
- TypeScript 锁定在 `~6.0.3`（`typescript-eslint` 8.x 不支持 7.x）。
- 自动化测试注意：用 `Start-Process -WindowStyle Hidden` 启动时，第一个提示框会被系统隐藏。正常启动不受影响。
- 模拟输入测试的坑（本机）：中文输入法会转换模拟键入的文字（改用粘贴）；Ctrl+` 是本机另一个截图/贴图工具的全局快捷键；新配置的 Edge 会弹出同步登录对话框（用 `--inprivate`）和"翻译此页"提示（先按 Esc）。

## 备注

- 开发环境：项目负责人使用 Windows。性能测试和手动测试必须在真实的 Windows 机器上进行。
- 仓库为公开仓库。不要提交任何密钥、Token 或个人数据。
