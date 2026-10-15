# 项目进度交接单

> 每次开发结束前更新本文件（见 `AGENTS.md` 的"结束流程"）。
> 只保留**当前**状态；已完成的历史以 `ROADMAP.md` 的勾选和 `git log` 为准。

**最后更新**：2026-10-15
**当前阶段**：M0 已完成；下一个里程碑是 M1 · 取词与词典（尚未开始）

---

## 当前状态

M0 的十项任务全部完成：monorepo、代码规范、CI、Tauri 托盘骨架、全局快捷键、设置读写、悬浮窗、悬浮窗生命周期、主窗口骨架、**性能测试（路线 A 已确认，D036）**。M0 收尾时评审了 `ARCHITECTURE.md` 第 3 节（仓库结构），已对照实际代码更新，并新增决定 D037（本地数据库放在 `lextrail-store`）、D038（dict-builder 用 Rust，词典数据不进仓库，构建时下载固定版本并校验哈希）、D039（M1 开始时先评审数据模型公共部分，倾向 HLC）。

代码现状（详见各文件）：
- `crates/lextrail-config`（D035）：`Settings`（`version`、`hotkeys.lookup`、`popup.mode`）+ `SettingsFile`（TOML、原子写入、损坏时备份为 `.corrupt` 并恢复默认、更新版本写的文件不覆盖）+ `SecretStore` 适配器（`WindowsCredentialStore` / `MemorySecretStore`）。17 个测试，其中 1 个真实读写 Windows 凭据管理器（用临时 key，测完删除）
- `apps/desktop/src-tauri`：
  - 启动时不创建任何窗口（不加载 WebView2），只有托盘；托盘图标左键单击打开主窗口，右键弹出菜单（"打开主窗口"、"退出 LexTrail"）
  - 单实例：第二次启动时唤起主窗口；关闭窗口不退出，只有菜单"退出"会退出
  - 主窗口（`main_window.rs`）：按需创建，960×640 逻辑像素、居中，页面加载完成后再显示；已打开时恢复最小化并置前；关闭即销毁，关闭后 WebView2 进程全部退出（已实测）。与悬浮窗共用同一个 WebView2 环境（D010，Tauri 默认共用数据目录）
  - 启动时从 `%APPDATA%\io.github.aspeternity.lextrail\settings.toml` 读设置（第一次启动会创建）；读取出问题时弹原生提示框，并用默认设置继续运行
  - 全局快捷键从设置读取（默认 Ctrl+Alt+X，D014）；注册失败时弹原生提示框，托盘提示文字也会改；按下后显示悬浮窗
  - 悬浮窗（`src/popup/`）：
    - 第一次按快捷键时才创建（只有托盘时不加载 WebView2），之后隐藏、复用，不重新加载（D010）
    - 生命周期（D009）：`popup/lifecycle.rs` 是纯状态机（Absent → Creating → Visible → Hidden → Suspending → Suspended，18 个测试），`popup/mod.rs` 执行它返回的动作
      - 隐藏 5 秒后（`SUSPEND_DELAY`）`SetIsVisible(false)` + `TrySuspend`；再次显示时 `Resume` + `SetIsVisible(true)` 后再显示窗口（`popup/webview.rs`）
      - "内存优先"（`popup.mode = "memory_saver"`）：隐藏即销毁窗口
      - 低内存：`memory.rs` 后台线程阻塞等待 `CreateMemoryResourceNotification(Low)`（不轮询；只在内存一直很低时每 60 秒复查一次）；悬浮窗不在屏幕上时销毁，正在显示时不动
      - debug 构建会在控制台打印每次状态转换（`popup: 事件 -> [动作]`）
    - 第一次显示等页面加载完成（`PageLoadEvent::Finished`）后再显示，避免出现白屏
    - 位置：默认在光标右下方 12 逻辑像素处，右边或下边放不下时翻到左边或上边，最后限制在光标所在显示器的工作区内（`popup/placement.rs`，纯函数，9 个测试，含负坐标副屏、比工作区还大等情况）
    - 关闭：页面里按 Esc 调用 `hide_popup` 命令；失去焦点（点击外部）时由 Rust 端隐藏
    - 尺寸暂定 380×260 逻辑像素，只有浅色主题，页面是占位内容
  - 提示框统一走 `notice::warn`（`tauri-plugin-dialog`，不加载 WebView）
- `apps/desktop/src`：Vite + Vue，`popup/`、`main/` 两个入口。悬浮窗页面只有 Esc 处理和占位内容（JS 0.6 kB + CSS 0.5 kB，Vue 运行时在公共 chunk 里）；主窗口页面还是占位内容
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

1. 进入 M1。第一步按 D039 评审 `ARCHITECTURE.md` 第 6 节的公共部分：同步元数据字段、HLC 时间戳格式、`lookups`（查询历史）表，评审结果写回第 6 节和 `DECISIONS.md`。
2. 然后创建 `lextrail-store`（D037），再按 `ROADMAP.md` 做取词、dict-builder（D038）、词典等任务。
3. M1、M2、M3、M6 都有"重测性能预算"任务，结果追加到 `docs/specs/m0-benchmark.md` 第 7 节。

## 遇到的问题 / 需要项目负责人决定

- 无需要现在决定的事项。服务端许可证写法和 `apps/server` 是否加入 Cargo workspace 推迟到 M4（见 `DECISIONS.md` 待确认事项）。
- 低优先级问题：冷启动后 0–100 ms 内按 Esc 可能无效（WebView 稍晚拿到键盘焦点），见报告 4.5 节。

已知情况（不需要处理）：
- GitHub Dependabot 有 1 个中等级别提醒，应该是 `glib 0.18.5`（RUSTSEC-2024-0429，unsound）。它只经由 Tauri 的 Linux GTK 托盘依赖引入，**Windows 构建里没有这个 crate**（已用 `cargo tree --target x86_64-pc-windows-msvc` 确认）。Tauri 2 锁定了 gtk 0.18，我们无法单独升级；做 macOS/Linux 支持时再看。可以在 GitHub 上把这条提醒标记为"不影响"（tolerable risk）。`cargo audit` 另有一条 `proc-macro-error` unmaintained 警告，同样只在 Linux 上用到。
- TypeScript 锁定在 `~6.0.3`：TypeScript 7.0 不被 `typescript-eslint` 8.x 支持（peer 要求 `<6.1.0`）。等生态支持 7.x 后再升级。
- 自动化测试注意：用 `Start-Process -WindowStyle Hidden` 启动时，第一个提示框会被系统隐藏（Windows 把启动参数用在进程第一个显示的窗口上）。正常启动不受影响。

## 备注

- 开发环境：项目负责人使用 Windows。M0 的性能测试必须在真实的 Windows 机器上进行。
- 仓库为公开仓库。不要提交任何密钥、Token 或个人数据。
