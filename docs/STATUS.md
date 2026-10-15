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

代码现状（详见各文件）：
- `crates/lextrail-core`：`text::sentence_around`（从段落切出选中文字所在的句子；处理缩写、小数、省略号、引号、中文标点、硬换行；位置提示无效时退回按文字查找）；`Hlc`（定宽小写文本、严格解析、按字符串比较即为先后）+ `HlcClock`（本地 `now`、收到远端 `receive`、计数器溢出借 1 ms、`restore` 回滚）+ 可注入的 `Clock`（`SystemClock` / 测试用 `ManualClock`）；`lookup` 模块：`LookupKind`、`CaptureMethod`、文本规范化（合并空白、最多 2000 字符）、30 秒去重判定。37 个测试
- `crates/lextrail-capture`（D041）：`TextCapture`（`snapshot` / `capture` / `capture_context`）、`CaptureOutcome` / `Unavailable`（取不到）与 `CaptureError`（出错，平台无关）；`Pipeline` 按顺序尝试各步骤（与平台无关，有测试）；`WindowsCapture`：`snapshot` 只做 Win32 调用（前台窗口、进程名、类名、标题、权限级别、`GetGUIThreadInfo` 的 hwndFocus / hwndCaret），UIA 在专用 MTA 线程上（预算 180 ms、卡死 5 秒放弃重建、最多 2 个被放弃的线程）；`capture_context` 目前返回 `None`（连按两次 Ctrl+C 时实现）。16 个测试；`examples/capture_probe.rs` 用于手动测试真实窗口
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
- `tools/bench`（`lextrail-bench`）：性能测试工具，`quick` 约 6 分钟，`long --wait-mins N` 测挂起 N 分钟后恢复；运行时会模拟按键、移动鼠标。报告和方法见 `docs/specs/m0-benchmark.md`
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

1. 按 `docs/specs/m1-capture-manual-tests.md` 在浏览器、PDF 阅读器、Word、VS Code、终端中手动测试 UIA 取词并记录结果（需要项目负责人在本机操作，或指定时间由 AI 驱动）。
2. 剪贴板兜底（D011）：完成后在 `WindowsCapture` 的 `Pipeline` 中加在 UIA 之后，并验证取词完成后悬浮窗仍能获得焦点（清单 6.10）。
3. 然后做连按两次 Ctrl+C（实现 `capture_context`）、dict-builder（D038）、词典等任务。
4. 桌面端接入查询历史时注意（项目负责人的要求）：第一次查询时用 `app_local_data_dir()` 按需打开 `Store`；`Store` 放进 `Mutex`，在后台线程读写；打开失败时用 `notice::warn` 提示，之后不记录历史、照常查词；加上"不记录查询历史"开关。
5. M1、M2、M3、M6 都有"重测性能预算"任务，结果追加到 `docs/specs/m0-benchmark.md` 第 7 节。

## 遇到的问题 / 需要项目负责人决定

- 无需要现在决定的事项。服务端许可证写法和 `apps/server` 是否加入 Cargo workspace 推迟到 M4（见 `DECISIONS.md` 待确认事项）。
- 需要关注：接入 UIA 取词后重测（`m0-benchmark.md` 第 7 节），冷启动中位数 351 ms、p90 381 ms，离 400 ms 预算只剩约 50 ms（M0 为 288 ms）；挂起内存 83.3 MiB、恢复 20.2 ms 仍达标。接入词典结果后要再测，必要时分析冷启动中取词和页面加载各占多少。
- 低优先级问题：冷启动后 0–100 ms 内按 Esc 可能无效（WebView 稍晚拿到键盘焦点），见报告 4.5 节。

已知情况（不需要处理）：
- GitHub Dependabot 有 1 个中等级别提醒，应该是 `glib 0.18.5`（RUSTSEC-2024-0429，unsound）。它只经由 Tauri 的 Linux GTK 托盘依赖引入，**Windows 构建里没有这个 crate**（已用 `cargo tree --target x86_64-pc-windows-msvc` 确认）。Tauri 2 锁定了 gtk 0.18，我们无法单独升级；做 macOS/Linux 支持时再看。可以在 GitHub 上把这条提醒标记为"不影响"（tolerable risk）。`cargo audit` 另有一条 `proc-macro-error` unmaintained 警告，同样只在 Linux 上用到。
- TypeScript 锁定在 `~6.0.3`：TypeScript 7.0 不被 `typescript-eslint` 8.x 支持（peer 要求 `<6.1.0`）。等生态支持 7.x 后再升级。
- 自动化测试注意：用 `Start-Process -WindowStyle Hidden` 启动时，第一个提示框会被系统隐藏（Windows 把启动参数用在进程第一个显示的窗口上）。正常启动不受影响。

## 备注

- 开发环境：项目负责人使用 Windows。M0 的性能测试必须在真实的 Windows 机器上进行。
- 仓库为公开仓库。不要提交任何密钥、Token 或个人数据。
