# 项目进度交接单

> 每次开发结束前更新本文件（见 `AGENTS.md` 的"结束流程"）。
> 只保留**当前**状态；已完成的历史以 `ROADMAP.md` 的勾选和 `git log` 为准。

**最后更新**：2026-10-15
**当前阶段**：M1 · 取词与词典（进行中：数据模型、HLC、`lextrail-store`、UIA 取词与悬浮窗焦点方案已完成）

---

## 当前状态

M0 的十项任务全部完成：monorepo、代码规范、CI、Tauri 托盘骨架、全局快捷键、设置读写、悬浮窗、悬浮窗生命周期、主窗口骨架、**性能测试（路线 A 已确认，D036）**。M0 收尾时评审了 `ARCHITECTURE.md` 第 3 节（仓库结构），已对照实际代码更新，并新增决定 D037（本地数据库放在 `lextrail-store`）、D038（dict-builder 用 Rust，词典数据不进仓库，构建时下载固定版本并校验哈希）、D039（M1 开始时评审数据模型公共部分）。

M1 第一项"评审数据模型公共部分"已完成（D039 改为 ✅）：同步元数据字段、HLC 时间戳格式（小写十六进制定宽文本，按字符串比较）、`lookups` 表的结构都已确定，写在 `ARCHITECTURE.md` 6.1–6.3；推迟到 M3/M4 的问题记在 6.5，并已加入 `ROADMAP.md`。本地表不带 `user_id`，`device_id` 存在数据库 `meta` 表中。

随后完成了 `lextrail-core` 的 HLC 和 `lextrail-store`，并新增决定 D040：数据库放在 `%LOCALAPPDATA%`（不放 Roaming），按需打开，以及 SQLite 的连接设置和迁移规则。

最近完成：`lextrail-capture` 统一接口 + Windows UIA 取词（D041），悬浮窗焦点方案 B（D042），`lextrail-core` 切句。快捷键取词已在记事本中端到端验证（冷启动、热显示、挂起后恢复都能直接获得焦点；慢取词时约 100 ms 后不抢焦点地显示"正在取词"；取词期间切换窗口会隐藏）。五类软件的手动测试还没做，清单见 `docs/specs/m1-capture-manual-tests.md`。

手动测试 1.1（Edge）发现并已修复（2026-10-16）：
- **悬浮窗始终空白**：仓库里没有 `capabilities/`，Tauri 2 的 ACL 拒绝了页面的 `listen()`，错误被 `void` 吞掉。之前在记事本里的"端到端验证"只看了 Rust 日志（`SetContent(Selection{...})`）和前台窗口标题，**没有看悬浮窗实际显示的内容**，所以没发现。已加 `capabilities/popup.json`（只授予 `core:event:allow-listen`），前端 listen/invoke 失败时打印错误，订阅失败会经 `popup_ready` 报给 Rust 日志。**以后的端到端验证必须截图亲眼看到内容，或用 `tools/e2e/popup-e2e.ps1` 自动检查**（用 UI Automation 读出悬浮窗文字核对）。
- **冷启动时序**：窗口就绪改为页面 `await listen` 后调用 `popup_ready`，不再以 `PageLoadEvent::Finished` 为准（生命周期事件改名 `PageReady`，补了测试）。
- **Chromium 第一次取词失败**：Chrome/Edge/Electron 第一次被 UIA 访问时才开启无障碍树；对 `Chrome_WidgetWin_*` 窗口在预算内每 30 ms 重试。全新 Edge 进程实测第一次即可取到（约 55–70 ms，`x2`）。
- **句子带列表符号**：`sentence_around` 去掉开头的 •、-、*、1.、(2)、a)、一、、① 等；行首的 "1." 不再被当作句末。
- 顺带修了悬浮窗中间多出一条横线（`#app` 没有高度）。

**第一轮手动测试（R1）全部完成**（2026-10-16），结果和问题表（R1-1 至 R1-11）见 `docs/specs/m1-capture-manual-tests.md`。修复分两个阶段，**第一阶段已完成**：
- A1（R1-5、R1-10）：悬浮窗在前台时再按快捷键，忽略并保留结果；桌面上取词显示"没有选中文字"。
- A2（R1-2）：获取焦点改为 `SetForegroundWindow`，被拒时检查 `IsHungAppWindow` 后用 `AttachThreadInput` 重试并立即解除挂接（`popup/activation.rs`），不再用 Tauri 模拟 Alt 的后备做法；获取焦点期间的前台变化不再隐藏悬浮窗；仍失败时不带焦点显示结果，右上角新增 × 关闭按钮（关闭方式见 ARCHITECTURE 9.3）。debug 版可用 `LEXTRAIL_DEBUG_SKIP_ACTIVATION=1` 模拟这种状态。
- A3（R1-1）：快捷键触发时 Alt 仍按着，就补发未分配的虚拟键 `0xE8`（`keyboard.rs`，见 D014 补充）。用 `tools/e2e/key-log.html` 确认：修复前程序只收到 Alt 按下/松开（X 的按下被快捷键拦截），修复后中间多了 `keyCode=232`。**VS Code 菜单栏被框住的现象我用模拟输入没能复现**（无论有无屏蔽键），需项目负责人用真实键盘复测（R2-1）。debug 版可用 `LEXTRAIL_DEBUG_NO_ALT_MASK=1` 关掉屏蔽键做对比。
- A4（R1-9）：提示文字不再提尚未实现的"连按两次 Ctrl+C"；做完该功能时加回（已写进 ROADMAP）。
- A5（R1-7）：`text::clean_control_chars` 过滤控制字符（`\u{7}` 等变空格），PDFium 的行末连字符 `\u{2}` 还原为 `-`；`text::resolve_line_end_hyphens` 已实现（去掉连字符后词典里查得到就合并），词典接入时调用（M1 任务已加）。
- A6（R1-8）：每次显示悬浮窗时按目标显示器当前缩放设置尺寸（无法自测，因需要改系统缩放设置，待 R2-11 复测）。
- A7（R1-6）：Chrome 的 PDF 阅读器实测选区正确（截图核对 5 个词），继续用 UIA；`[chromium_pdf]` 只针对 Edge。

第二轮复测清单（R2-1 至 R2-12）已写在手动测试清单里，**等项目负责人复测**。

手动测试第 1 节（浏览器）通过（Firefox 未安装）。第 2 节（Edge 内置 PDF）发现并处理（2026-10-16）：
- **选中文字错位**：用 `examples/uia_dump.rs` 打印原始数据确认，是 **Edge 的 PDF 阅读器本身**报告的选区范围错位（选区 range 的 `GetText()` 就是错的，如 "between" → "ween di"），不是 LexTrail 用偏移截取文字；报告的"段落"也会从段落中间开始（句子缺开头同一原因）。无法判断哪次可信，所以检测到 Chromium PDF 时 UIA 返回 `unsupported [chromium_pdf]`，等剪贴板兜底接手。补了"偏移与文字不一致"的单元测试（原有逻辑已会退回按文字查找）。
- **刚打开 PDF 时 no_selection**：PDF 现在一律不走 UIA。评估后**不在 no_selection 时重试**（实测重试拿不到早于无障碍树的选区，只会耗尽预算），留到剪贴板兜底时决定是否用剪贴板确认（已加入 ROADMAP）。
- 句子开头的省略号、过长上下文的截取：已加入 ROADMAP（低优先级）。

代码现状（详见各文件）：
- `crates/lextrail-core`：`text::sentence_around`（从段落切出选中文字所在的句子；处理缩写、小数、省略号、引号、中文标点、硬换行、开头的列表符号；位置提示无效时退回按文字查找）；`Hlc`（定宽小写文本、严格解析、按字符串比较即为先后）+ `HlcClock`（本地 `now`、收到远端 `receive`、计数器溢出借 1 ms、`restore` 回滚）+ 可注入的 `Clock`（`SystemClock` / 测试用 `ManualClock`）；`lookup` 模块：`LookupKind`、`CaptureMethod`、文本规范化（合并空白、最多 2000 字符）、30 秒去重判定；`text::clean_control_chars`、`resolve_line_end_hyphens`。46 个测试
- `crates/lextrail-capture`（D041）：`TextCapture`（`snapshot` / `capture` / `capture_context`）、`CaptureOutcome` / `Unavailable`（取不到）与 `CaptureError`（出错，平台无关）；`Pipeline` 按顺序尝试各步骤（与平台无关，有测试）；`WindowsCapture`：`snapshot` 只做 Win32 调用（前台窗口、进程名、类名、标题、权限级别、`GetGUIThreadInfo` 的 hwndFocus / hwndCaret），UIA 在专用 MTA 线程上（预算 180 ms、卡死 5 秒放弃重建、最多 2 个被放弃的线程）；`capture_context` 目前返回 `None`（连按两次 Ctrl+C 时实现）；Chromium 窗口在预算内重试，Edge PDF 不走 UIA，桌面直接返回没有选中文字。21 个测试；`examples/capture_probe.rs` 用于手动测试真实窗口。`capture_traced` 返回 `CaptureTrace`（每一步的方式、结果、耗时、焦点元素来源，不含文字）；debug 构建每次取词打印一行，如 `capture #2: captured | uia=captured 3.1ms (uia_focus); total 3.1ms`
- `crates/lextrail-store`（D037、D040）：`Store::open_in_dir`（创建目录、`lextrail.db`）/ `open_in_memory_with_clock`；`user_version` 迁移（`src/migrations/0001_init.sql`：`meta`、`lookups`，STRICT 表）；版本更高时拒绝打开且不改文件；WAL、`synchronous=NORMAL`、`foreign_keys`、`busy_timeout` 5 秒；`meta` 存 `device_id`（UUIDv7）和最近一次 HLC，与数据写入同一事务；`record_lookup`（规范化、去重、空查询忽略）、`delete_lookup`（软删除）、`lookup`、`recent_lookups`。20 个测试。**桌面端尚未接入**
- `crates/lextrail-config`（D035）：`Settings`（`version`、`hotkeys.lookup`、`popup.mode`）+ `SettingsFile`（TOML、原子写入、损坏时备份为 `.corrupt` 并恢复默认、更新版本写的文件不覆盖）+ `SecretStore` 适配器（`WindowsCredentialStore` / `MemorySecretStore`）。17 个测试，其中 1 个真实读写 Windows 凭据管理器（用临时 key，测完删除）
- `apps/desktop/src-tauri`：
  - 启动时不创建任何窗口（不加载 WebView2），只有托盘；托盘图标左键单击打开主窗口，右键弹出菜单（"打开主窗口"、"退出 LexTrail"）
  - 单实例：第二次启动时唤起主窗口；关闭窗口不退出，只有菜单"退出"会退出
  - 主窗口（`main_window.rs`）：按需创建，960×640 逻辑像素、居中，页面加载完成后再显示；已打开时恢复最小化并置前；关闭即销毁，关闭后 WebView2 进程全部退出（已实测）。与悬浮窗共用同一个 WebView2 环境（D010，Tauri 默认共用数据目录）
  - 启动时从 `%APPDATA%\io.github.aspeternity.lextrail\settings.toml` 读设置（第一次启动会创建）；读取出问题时弹原生提示框，并用默认设置继续运行
  - 全局快捷键从设置读取（默认 Ctrl+Alt+X，D014）；注册失败时弹原生提示框，托盘提示文字也会改；按下后调用 `popup::lookup`：先 `snapshot`，再准备悬浮窗，同时在后台线程取词
  - 悬浮窗（`src/popup/`）：
    - 第一次按快捷键时才创建（只有托盘时不加载 WebView2），之后隐藏、复用，不重新加载（D010）
    - 生命周期（D009）：`popup/lifecycle.rs` 是纯状态机（Absent → Creating → Visible → Hidden → Suspending → Suspended，18 个测试），`popup/mod.rs` 执行它返回的动作
      - 隐藏 5 秒后（`SUSPEND_DELAY`）`SetIsVisible(false)` + `TrySuspend`；再次显示时 `Resume` + `SetIsVisible(true)` 后再显示窗口（`popup/webview.rs`）
      - "内存优先"（`popup.mode = "memory_saver"`）：隐藏即销毁窗口
      - 低内存：`memory.rs` 后台线程阻塞等待 `CreateMemoryResourceNotification(Low)`（不轮询；只在内存一直很低时每 60 秒复查一次）；悬浮窗不在屏幕上时销毁，正在显示时不动
      - debug 构建会在控制台打印每次状态转换（`popup: 事件 -> [动作]`、`request: ...`、`capture #N: ...`）
    - 一次查词（D042）：`popup/request.rs` 是纯状态机（8 个测试），决定何时显示、显示什么、何时获取焦点；`popup/foreground.rs` 用 `SetWinEventHook(EVENT_SYSTEM_FOREGROUND)` 在取词期间监视前台窗口；`popup/content.rs` 把取词结果转成发给页面的 `popup:content` 事件。debug 构建可用环境变量 `LEXTRAIL_DEBUG_CAPTURE_DELAY_MS` 人为让取词变慢
    - 第一次显示等页面加载完成（`PageLoadEvent::Finished`）后再显示，避免出现白屏；取词期间以 `SW_SHOWNOACTIVATE` 显示，取词完成后 `SetForegroundWindow`（失败时退回 Tauri 的 `set_focus`）
    - 位置：默认在光标右下方 12 逻辑像素处，右边或下边放不下时翻到左边或上边，最后限制在光标所在显示器的工作区内（`popup/placement.rs`，纯函数，9 个测试，含负坐标副屏、比工作区还大等情况）
    - 关闭：页面里按 Esc 调用 `hide_popup` 命令；失去焦点（点击外部）时由 Rust 端隐藏
    - 尺寸暂定 380×260 逻辑像素，只有浅色主题；页面显示取到的文字、所在句子和来源软件，或"没有选中文字""无法读取以管理员身份运行的程序"等提示（词典结果待接入）
  - 提示框统一走 `notice::warn`（`tauri-plugin-dialog`，不加载 WebView）
- `apps/desktop/src`：Vite + Vue，`popup/`、`main/` 两个入口。悬浮窗页面监听 `popup:content` 事件显示内容（`popup/content.ts` 与 Rust 端的 `Content` 对应），Esc 关闭；前端还没有测试框架；主窗口页面还是占位内容
- `tools/e2e/popup-e2e.ps1`：悬浮窗内容的端到端自动检查（真实鼠标、真实快捷键、全新 Edge，用 UI Automation 读出悬浮窗文字核对，并保存截图），用法见手动测试清单第 0 节
- `tools/bench`（`lextrail-bench`）：性能测试工具，`quick` 约 6 分钟，`long --wait-mins N` 测挂起 N 分钟后恢复；运行时会模拟按键、移动鼠标。启动应用时设置 `LEXTRAIL_BENCH_TIMINGS`，应用把每次查词的分项时间（只有数字）写进该文件，bench 据此分开报告取词耗时和窗口显示耗时。报告和方法见 `docs/specs/m0-benchmark.md`
- CI（`.github/workflows/ci.yml`）：ubuntu 跑格式、ESLint、类型检查；Windows 跑 clippy、测试，并用 `tauri build --no-bundle` 做 release 构建

正式性能测试结果（`docs/specs/m0-benchmark.md`，口径见 D036）：托盘 1.6 MiB；挂起 15 秒 82.8 MiB、2 小时后 13.0 MiB；冷启动显示 288 ms；挂起后恢复 11.7 ms，挂起 2 小时后恢复 44.7 ms；空闲 CPU ≈ 0。全部达标，但刚挂起时距离 100 MB 只剩约 17 MB，后续里程碑都要重测。
"内存优先"模式：隐藏后 0.5 秒内 WebView2 进程全部退出，每次显示都是冷启动（约 270–290 ms）。
以上功能都在本机手动验证过；托盘图标已由项目负责人目测确认。

常用命令：
- `pnpm check`：格式、lint、类型检查、全部测试
- `pnpm --filter @lextrail/desktop tauri dev`：开发运行桌面端
- `pnpm --filter @lextrail/desktop tauri build --no-bundle`：构建真正的 release 版。**注意**：直接 `cargo build --release` 得到的仍是开发模式（加载 `devUrl` http://localhost:1420），手动测试和性能测试都必须用 `tauri build`
- `cargo audit`：依赖漏洞检查（本机已装 cargo-audit）
- `cargo run -p lextrail-bench --release -- quick`：性能测试（先用 `tauri build` 构建；运行时不要操作电脑）

## 正在进行

- 无。

## 下一步

1. 项目负责人按手动测试清单的"第一阶段修复后的复测（R2）"复测，结果发回后处理。
2. **第二阶段：剪贴板兜底**（D011），按已确认的计划：
   - B1：修饰键松开后模拟 Ctrl+C（终端用 Ctrl+Insert，Windows Terminal 和 conhost 分别实测），用剪贴板序列号确认更新；恢复原剪贴板时写清恢复哪些格式，写回的内容加"不进入剪贴板历史"的标记（目标程序复制产生的那条历史无法避免，写进已知限制）。作为 `Pipeline` 第二步，排在 UIA 之后。
   - B2：UIA 返回"没有选中文字"时，只对 UIA 不可靠的程序（Chromium 系窗口类名、PDF 等）用剪贴板确认，记事本、Word 等可靠程序不做；结论记入 DECISIONS（新编号）。注意 VS Code 编辑器在没有选区时 Ctrl+C 会复制整行，可用剪贴板中的 `vscode-editor-data` 格式（`isFromEmptySelection`）排除。
   - B3：查 VS Code 编辑器和集成终端 UIA 返回 `no_selection` 的原因（`editor.accessibilitySupport`），不改用户设置，由剪贴板兜底覆盖；集成终端只在有选区时 Ctrl+C 才是复制。
   - B4：验证剪贴板路径下悬浮窗仍能获得焦点（清单 6.10），用 e2e 脚本加截图。
   - 做完后同样给出只需复测的清单。用模拟输入调查前后都要告诉项目负责人。
3. 然后做连按两次 Ctrl+C（实现 `capture_context`）、dict-builder（D038）、词典等任务。
4. 桌面端接入查询历史时注意（项目负责人的要求）：第一次查询时用 `app_local_data_dir()` 按需打开 `Store`；`Store` 放进 `Mutex`，在后台线程读写；打开失败时用 `notice::warn` 提示，之后不记录历史、照常查词；加上"不记录查询历史"开关。
5. M1、M2、M3、M6 都有"重测性能预算"任务，结果追加到 `docs/specs/m0-benchmark.md` 第 7 节。

## 遇到的问题 / 需要项目负责人决定

- 无需要现在决定的事项。服务端许可证写法和 `apps/server` 是否加入 Cargo workspace 推迟到 M4（见 `DECISIONS.md` 待确认事项）。
- 需要关注：冷启动两次重测分别为 351 ms 和 281 ms（预算 400 ms），波动较大。分项计时（`m0-benchmark.md` 7.1）显示冷启动几乎全部是窗口创建和页面加载（约 274 ms），取词约 13 ms 且与之并行；热显示和挂起后恢复的显示时间由取词决定（7–11 ms）。接入词典结果后要再测，看趋势时多测几次。
- 低优先级问题：冷启动后 0–100 ms 内按 Esc 可能无效（WebView 稍晚拿到键盘焦点），见报告 4.5 节。

已知情况（不需要处理）：
- GitHub Dependabot 有 1 个中等级别提醒，应该是 `glib 0.18.5`（RUSTSEC-2024-0429，unsound）。它只经由 Tauri 的 Linux GTK 托盘依赖引入，**Windows 构建里没有这个 crate**（已用 `cargo tree --target x86_64-pc-windows-msvc` 确认）。Tauri 2 锁定了 gtk 0.18，我们无法单独升级；做 macOS/Linux 支持时再看。可以在 GitHub 上把这条提醒标记为"不影响"（tolerable risk）。`cargo audit` 另有一条 `proc-macro-error` unmaintained 警告，同样只在 Linux 上用到。
- TypeScript 锁定在 `~6.0.3`：TypeScript 7.0 不被 `typescript-eslint` 8.x 支持（peer 要求 `<6.1.0`）。等生态支持 7.x 后再升级。
- 自动化测试注意：用 `Start-Process -WindowStyle Hidden` 启动时，第一个提示框会被系统隐藏（Windows 把启动参数用在进程第一个显示的窗口上）。正常启动不受影响。

## 备注

- 开发环境：项目负责人使用 Windows。M0 的性能测试必须在真实的 Windows 机器上进行。
- 仓库为公开仓库。不要提交任何密钥、Token 或个人数据。
