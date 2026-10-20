# 项目进度交接单

> 每次开发结束前更新本文件（见 `AGENTS.md` 的"结束流程"）。
> 只保留**当前**状态；已完成的历史以 `ROADMAP.md` 的勾选和 `git log` 为准。

**最后更新**：2026-10-21（连按两次 Ctrl+C 第一阶段完成，分支 `feat/double-copy`）
**当前阶段**：M1 · 取词与词典（进行中）。取词（UIA + 剪贴板兜底 + 悬浮窗焦点）经 R1–R4 手动测试验收；词典（`tools/dict-builder`、`lextrail-dict`、悬浮窗词典结果）完成，R5 通过；查询历史已接入桌面端。都已合并到 `main`。M1 剩下：连按两次 Ctrl+C、快捷键设置界面、前端测试等（见 `ROADMAP.md`）。

---

## 当前状态

M0 全部完成（路线 A 已确认，D036）。M1 已完成：

- **数据模型公共部分**评审（D039）：同步元数据、HLC 时间戳格式、`lookups` 表，见 `ARCHITECTURE.md` 6.1–6.3，推迟事项在 6.5。
- **`lextrail-core` 的 HLC** 与 **`lextrail-store`**（D037、D040：数据库在 `%LOCALAPPDATA%`，按需打开）。桌面端已接入查询历史（`history.rs`）：第一次记录时打开数据库，结果发给悬浮窗之后在取词线程上写入；"不记录查询历史"用设置文件 `[history] record = false`，界面开关随快捷键设置界面一起做。
- **取词**（D041）：统一接口 + Windows UIA，在专用 MTA 线程上运行；Chromium 窗口第一次取词时在预算内重试；Edge 的 PDF 阅读器报告的选区会错位，所以不走 UIA（`[chromium_pdf]`），Chrome 的 PDF 正常走 UIA。
- **悬浮窗焦点方案 B**（D042）：取词期间不抢焦点，超过约 100 ms 才显示"正在取词…"（按住快捷键的时间不算，松开时重新计时；按住超过约 1 秒显示"松开快捷键后取词…"，D042 补充），取词完成后获取焦点（`SetForegroundWindow`，被拒时用 `AttachThreadInput`；不再用 Tauri 模拟 Alt 的后备做法）；仍失败时不带焦点显示结果，可用右上角 × 关闭；悬浮窗在前台时再按快捷键会被忽略；每次显示时按目标显示器当前缩放设置尺寸。
- **快捷键屏蔽键**（D014 补充）：快捷键触发时 Alt 仍按着，就对仍按着的 Ctrl（其次 Shift）补发一次按下、不发松开，避免 VS Code 等程序把它当成单独按 Alt 而激活菜单栏；原来的 `0xE8` 会让 Windows Terminal 清掉选区（R3-P1），现在只在只有 Alt 时使用。补发期间在单独线程上装几毫秒的低级键盘钩子，用户恰好在补发前松开时再补发松开，避免 Ctrl 卡住。执行顺序：屏蔽键 → 记录前台窗口 → 启动取词线程 → 请求显示悬浮窗（第一次时创建窗口）。
- **剪贴板兜底**（D043）：等松开快捷键再复制，最多等 3 秒，超过时提示"请松开快捷键后再试"（D043 补充）；UIA 不支持、超时或出错时运行；UIA 说"没有选中"时只对 Chromium 系窗口用剪贴板确认。终端（Windows Terminal、conhost、VS Code 集成终端）用 Ctrl+Insert；VS Code 的空选区复制（整行）识别为"没有选中"；复制前保存剪贴板、复制后写回，写回的内容不进剪贴板历史。已在记事本、Edge PDF、conhost、Windows Terminal、VS Code 编辑器和终端中实测（截图核对）。
- **文字处理**（`lextrail-core::text`）：`sentence_around` 切句（处理缩写、小数、省略号、引号、中文标点、硬换行、开头的列表符号）；`clean_control_chars` 过滤控制字符，PDFium 的行末连字符 `\u{2}` 还原为 `-`；`resolve_line_end_hyphens`（词典里查得到就合并，如 compu-tation）已写好，**等词典接入时调用**。

手动测试：清单和全部结果在 `docs/specs/m1-capture-manual-tests.md`。
- 第一轮（R1，2026-10-16）：发现 11 个问题（R1-1 至 R1-11），已分两阶段处理。
- 第二轮（R2，2026-10-16）：第一阶段修复后复测，12 项全部通过，第一阶段验收。
- 第三轮（R3，2026-10-16）：剪贴板兜底后的复测，13 项通过；R3-9（Windows Terminal）失败，另发现 R3-P2 至 R3-P4，均已修复，处理说明见清单"第三轮发现的问题"。R3-P3 的原因：第一次查词时创建悬浮窗阻塞了快捷键线程，取词线程在那之后才启动，用户已经松手，所以几乎不用等（复制本身没有问题）。
- 第四轮（R4，2026-10-20）：R4-1 至 R4-12 全部通过；其中发现的 R4-P1（悬浮窗挂起后卡死）、R4-P2（VS Code 中按住快捷键超过 3 秒提示不对）已修复并复测通过。取词部分验收。
- 第五轮（R5，2026-10-20）：悬浮窗词典结果，R5-1 至 R5-18 通过；R5-P1（Edge PDF "English to-German" 拆分）、R5-P2（没有标记的变形词条排在原形后）已修复。

代码现状（详见各文件）：
- `crates/lextrail-core`：HLC（`Hlc`、`HlcClock`、可注入的 `Clock`）；`lookup`（`LookupKind`、`CaptureMethod`、`normalize_text`、30 秒去重）；`text`（见上，另有 `join_unmarked_line_breaks`、`starts_sentence`）。48 个测试
- `crates/lextrail-capture`（D041、D043）：`TextCapture`（`snapshot` / `capture` / `capture_traced` / `capture_context`，最后一个目前返回 `None`，第二阶段实现）；`Pipeline` 按顺序运行各步骤，步骤间用 `StepContext` 传递信息，UIA 的"没有选中"默认就是最终结果；`clipboard_rules.rs`（剪贴板兜底的规则，与平台无关）；`double_copy.rs`（连按两次 Ctrl+C 的判定，与平台无关）；`windows/`：`window.rs`（快照）、`uia.rs`（UIA 步骤）、`clipboard.rs`（剪贴板步骤）。45 个测试，其中 `clipboard_round_trip` 默认忽略（会改动真实剪贴板，需要时用 `cargo test -p lextrail-capture -- --ignored clipboard_round_trip`）。`examples/capture_probe.rs`（手动测试真实窗口）、`examples/uia_dump.rs`（打印原始 UIA 数据）。`capture_traced` 接收进度回调（`CaptureProgress`：等松开快捷键 / 已松开），步骤结果带 `key_wait`
- `crates/lextrail-store`（D037、D040）：SQLite、`user_version` 迁移、`meta` / `lookups` 表、`record_lookup` 等。20 个测试
- `crates/lextrail-config`（D035）：设置文件与凭据管理器；`[history] record`（"不记录查询历史"开关）。17 个测试
- `apps/desktop/src-tauri`：托盘、单实例、主窗口（占位）、设置、全局快捷键（`hotkey.rs` + `keyboard.rs` 屏蔽键与防卡键钩子，10 个测试）、悬浮窗（`popup/`：`lifecycle.rs` 窗口生命周期 20 个测试，`request.rs` 一次查词 15 个测试，`activation.rs` 获取焦点，`foreground.rs` 前台窗口监视，`content.rs` 内容事件，`placement.rs` 位置，`timing.rs` bench 计时）。悬浮窗页面先订阅内容事件再调用 `popup_ready`，之后才显示；权限在 `capabilities/popup.json`（只有 `core:event:allow-listen`）。共 77 个测试
- `apps/desktop/src`：悬浮窗页面（`popup/`：`content.ts`、`ipc.ts`、`App.vue`），主窗口占位。**前端还没有测试框架**
- `tools/e2e/`：`popup-e2e.ps1`（悬浮窗内容端到端检查：真实鼠标、真实快捷键、全新 Edge，用 UI Automation 读出悬浮窗文字核对并截图；`-Pdf` 走剪贴板路径；`-HoldMs` 按住 Ctrl+Alt 指定时间）、`stuck-keys.ps1`（屏蔽键防卡键压力测试）、`list-item.html`、`key-log.html`（记录页面收到的键盘事件）
- `tools/bench`：性能测试；应用在 `LEXTRAIL_BENCH_TIMINGS` 下写出分项计时；`--lookup-text TEXT` 在词典打开的状态下测（m0-benchmark 7.3）。14 个测试
- `crates/lextrail-dict`（D044–D046）：查询、词形还原、输入分类、`is_word`（合并断词用，含完整版词头表 `extra_words`）、`view`（显示用的结果）。`examples/view.rs` 打印某个词的显示结果（JSON）
- `apps/desktop/src-tauri/src/dictionary.rs`：词典常驻服务（第一次取到文字时打开）；`popup/content.rs` 合并断词、分类、查词，并给出查询历史要记的内容（`history::Record`）；悬浮窗页面 `popup/DictResult.vue`
- `apps/desktop/src-tauri/src/history.rs`：查询历史（第一次记录时打开数据库，结果发给悬浮窗之后在取词线程上写入；打开失败提示一次，本次运行不再记录）。debug 版每次取词多打印一行 `capture #N: history recorded|repeat|skipped 5.3ms`

debug 版开关（只在 debug 构建中有效，用法见手动测试清单第 0 节）：`LEXTRAIL_DEBUG_CAPTURE_DELAY_MS`（让取词变慢）、`LEXTRAIL_DEBUG_SKIP_ACTIVATION`（不获取焦点）、`LEXTRAIL_DEBUG_NO_ALT_MASK`（关掉屏蔽键）、`LEXTRAIL_DEBUG_ALT_MASK_E8`（屏蔽键总用旧的 `0xE8`，用来对比）、`LEXTRAIL_DEBUG_CLIPBOARD_ONLY`（跳过 UIA，只走剪贴板）、`LEXTRAIL_DEBUG_CONFIG_DIR` / `LEXTRAIL_DEBUG_DATA_DIR`（设置文件、数据库改用指定目录；`popup-e2e.ps1`、`stuck-keys.ps1` 已默认用临时目录。**自动测试一律用临时目录，不碰项目负责人的真实设置和数据库**）。debug 版每次取词在控制台打印一行，如 `capture #2: captured | start +7.6ms; uia=no_selection 16.8ms (uia_focus); clipboard=captured 160.3ms keys=150.1ms [ctrl_insert]; total 177.1ms`（不含取到的文字；`start` 是从按下快捷键到开始取词，`keys` 是等松开快捷键的时间）；每次补发屏蔽键打印 `hotkey: alt mask: ...`。

性能（`docs/specs/m0-benchmark.md` 第 7 节，口径见 D036）：最近一次（2026-10-20，查询历史接入后，7.4 节）托盘 1.6 MiB、挂起 84.9 MiB（查过词，词典和数据库都打开；比 7.3 节多约 2.3 MiB，在运行间的波动范围内）、冷启动 264 ms、热显示 26 ms、挂起后恢复 32 ms，全部达标。**bench 要在普通终端窗口（如 Windows Terminal）中运行**：从 Claude 桌面应用等 Chromium 系窗口里启动时，前台是它，每次多等约 400 ms 的剪贴板确认，结果不可比。

常用命令：
- `cargo run -p lextrail-dict-builder --release`：生成词典 `data/build/lextrail-dict-core.db`（格式版本 2）。**编译桌面程序之前必须先生成**（它是 Tauri 资源，缺失时 `tauri-build` 报错）
- `cargo run -p lextrail-dict --release --example view -- data/build/lextrail-dict-core.db better self-attention`：看某个词在悬浮窗里会显示什么
- `pnpm check`：格式、lint、类型检查、全部测试
- `pnpm --filter @lextrail/desktop tauri build --debug --no-bundle`：带日志的 debug 版（`target\debug\lextrail-desktop.exe`，在终端里运行看日志）
- `pnpm --filter @lextrail/desktop tauri build --no-bundle`：真正的 release 版。**不要用** `cargo build --release`（会加载开发服务器页面）
- `powershell -NoProfile -ExecutionPolicy Bypass -File tools/e2e/popup-e2e.ps1 -App target/debug/lextrail-desktop.exe [-Pdf] [-HoldMs 2000]`：悬浮窗端到端检查（会移动鼠标、模拟按键）
- `powershell -NoProfile -ExecutionPolicy Bypass -Command "& ./tools/e2e/stuck-keys.ps1 -App target/debug/lextrail-desktop.exe -GapsMs 0,0,1,2,3 -Rounds 24"`：屏蔽键防卡键压力测试（模拟按键；数组参数要用 `-Command` 传）
- `cargo run -p lextrail-capture --example capture_probe -- 3`：3 秒后对前台窗口取词并打印结果
- `cargo run -p lextrail-bench --release -- quick`：性能测试（先 `tauri build`；运行时不要操作电脑）
- `cargo audit`：依赖漏洞检查

## 正在进行

- **连按两次 Ctrl+C**（分支 `feat/double-copy`，spec `docs/specs/m1-double-copy.md`，D012 补充）：
  - 第一阶段完成：spec；`lextrail-capture::double_copy`（`DoubleCopyDetector`：50 ms 内算同一次复制、50–500 ms 算连按、两次必须是相同文字、VS Code 整行复制算"没有选中"、自己取词期间到写回后 300 ms 的更新忽略），11 个单元测试。
  - 终端实测（spec 第四节）：Windows Terminal、VS Code 终端里第一次 Ctrl+C 复制并清除选区，第二次中断 `ping -t`，剪贴板没有第二次更新——终端里永远不会触发，还会中断程序。
  - spec 里两条"待确认"：带 `ExcludeClipboardContentFromMonitorProcessing` 标记（密码管理器）的更新忽略；LexTrail 自己的窗口在前台时忽略。开关位置 `[hotkeys] double_ctrl_c`。
  - 下一步：第二阶段（Windows 剪贴板监听、`capture_context`、桌面端接入）。

## 下一步

1. 连按两次 Ctrl+C（D012，默认关闭），分支 `feat/double-copy`，分三个阶段提交，每个阶段更新本文件：① spec + 连按判定逻辑和单元测试（已完成）；② Windows 剪贴板监听和 `capture_context`；③ e2e 和文档。完成后把这个用法加回提示文字（R1-9）。之后做快捷键设置界面。
2. 查询历史的界面（列表、删除）不在 M1；"不记录查询历史"的界面开关随快捷键设置界面一起做。

待跟进（已在 ROADMAP 中，或在此记录）：
- **B3 没有结论**：VS Code 编辑器里 UIA 报告"没有选中"的原因未确认。把 `editor.accessibilitySupport` 设为 `on` 后，我用模拟按键没能做出选区（屏幕阅读器模式下 Ctrl+Shift+→ 的行为不同），这次测试不成立。剪贴板兜底已覆盖 VS Code，且不改用户设置，所以暂不继续；如需要，可在 `on` 模式下手动选词，用 `capture_probe` 看 UIA 是否取到。
- **获取焦点的后备路径未在真实环境中触发过**（R2-5）：`AttachThreadInput` 重试只经代码审查；"被拒后不带焦点显示"只用模拟开关验证过。以后如在日志里看到 `Attached` 或 `Refused`，记下当时的场景。
- 前端测试框架（Vitest）还没引入（M1 任务）。
- 低优先级：Windows Terminal 跨行选中时句子带上提示符（R1-10）；句首省略号、过长上下文截取（R1-11）。
- 依赖漏洞提示 RUSTSEC-2024-0429（`glib` 的 `VariantStrIter`）：只影响 Linux；`glib` 0.18.5 是 Tauri 在 Linux 上的依赖，修复要 0.20，我们自己升级不了。GitHub 上的 Dependabot 提醒目前仍是打开状态（2026-10-20 用 `gh api` 查过）。**等 Tauri 升级到 glib 0.20 以上时再看。**
- 低优先级：冷启动后 0–100 ms 内按 Esc 可能无效（`m0-benchmark.md` 4.5 节）。
- 第一次显示的悬浮窗比之后高（debug 版、1.5 倍缩放：592×446，之后 592×403），2026-10-20 截图时发现；窗口大小只由 Rust 设置，这次没有改，应是原来就有，原因未查（ROADMAP 已记）。
- bench 运行环境：2026-10-20 有几次运行中取词约 430 ms（`main` 也出现），应是 bench 终端没有一直在前台；看 bench 结果时先看"capture (in app)"是否正常（约 20–35 ms）。
- 用模拟输入做调查或测试之前和之后都要告诉项目负责人（项目负责人的要求）；端到端验证必须截图看到悬浮窗内容或用 `popup-e2e.ps1`，只看日志不算。
- 模拟按键之前先确认前台窗口是测试要的那个，不是就不发（多余的按键会落到项目负责人当时在前台的程序里）；截图只截悬浮窗、提示框或测试窗口自己的区域（用窗口句柄或 UI Automation 取位置），不截全屏（项目负责人的要求）。

## 遇到的问题 / 需要项目负责人决定

- **（新发现，2026-10-20）屏蔽键压力测试又出现左 Ctrl 卡住**：`stuck-keys.ps1` 在间隔 0 ms（松开与补发同时）时约 1/4 的按键之后左 Ctrl 仍是按下状态（40 次中 6 次、20 次中 5 次；1–20 ms 的间隔都正常）。日志里这些按键钩子看到的顺序是 `A2v* A2^`（补发在前、真实松开在后，按设计不该卡住），但 700 ms 后 `GetAsyncKeyState` 仍报告按下。屏蔽键代码 10-17 以来没有改过，当时 120 次没有卡住；原因未查。`popup-e2e.ps1`（WScript `SendKeys "^%x"`）也出现过：钩子看到 `A4^ A2v*`，没有看到真实的 Ctrl 松开，结束时左 Ctrl 仍按着（之后 `-Pdf` 那次失败就是因为它）。真实键盘松开没有这么快，影响可能小，但卡住的 Ctrl 会作用到用户接下来的按键。**需要项目负责人决定：先查这个，还是先做连按两次 Ctrl+C。** 两个 e2e 脚本现在退出前会松开仍按着的修饰键（只发松开）并算作失败。
- 其他无需要现在决定的事项。服务端许可证写法和 `apps/server` 是否加入 Cargo workspace 推迟到 M4（见 `DECISIONS.md` 待确认事项）。
- 需要关注：冷启动两次重测分别为 351 ms 和 281 ms（预算 400 ms），波动较大；冷启动几乎全部是窗口创建和页面加载（约 257–274 ms）；取词原来排在创建窗口之后（R3-P3），现已改为先启动取词线程，两者并行。2026-10-20 接入词典后三次为 301–376 ms，同时测的 `main` 为 390 ms（m0-benchmark 7.3），继续观察。

已知情况（不需要处理）：
- GitHub Dependabot 有 1 个中等级别提醒（`glib 0.18.5`，RUSTSEC-2024-0429），仍是打开状态，只经由 Tauri 的 Linux GTK 依赖引入，Windows 构建里没有这个 crate。`cargo audit` 另有一条 `proc-macro-error` unmaintained 警告，同样只在 Linux 上用到。
- TypeScript 锁定在 `~6.0.3`（`typescript-eslint` 8.x 不支持 7.x）。
- 自动化测试注意：用 `Start-Process -WindowStyle Hidden` 启动时，第一个提示框会被系统隐藏。正常启动不受影响。
- 模拟输入测试的坑（本机）：中文输入法会转换模拟键入的文字（改用粘贴）；Ctrl+` 是本机另一个截图/贴图工具的全局快捷键；新配置的 Edge 会弹出同步登录对话框（用 `--inprivate`）和"翻译此页"提示（先按 Esc）。

## 备注

- 开发环境：项目负责人使用 Windows。性能测试和手动测试必须在真实的 Windows 机器上进行。
- 仓库为公开仓库。不要提交任何密钥、Token 或个人数据。
