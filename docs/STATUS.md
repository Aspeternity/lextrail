# 项目进度交接单

> 每次开发结束前更新本文件（见 `AGENTS.md` 的"结束流程"）。
> 只保留**当前**状态；已完成的历史以 `ROADMAP.md` 的勾选和 `git log` 为准。

**最后更新**：2026-10-15
**当前阶段**：M0 · 项目骨架与性能原型（已完成 7 / 10 项）

---

## 当前状态

M0 已完成：monorepo、代码规范、CI、Tauri 托盘骨架、全局快捷键、设置读写、悬浮窗。

代码现状（详见各文件）：
- `crates/lextrail-config`（D035）：`Settings`（`version`、`hotkeys.lookup`、`popup.mode`）+ `SettingsFile`（TOML、原子写入、损坏时备份为 `.corrupt` 并恢复默认、更新版本写的文件不覆盖）+ `SecretStore` 适配器（`WindowsCredentialStore` / `MemorySecretStore`）。17 个测试，其中 1 个真实读写 Windows 凭据管理器（用临时 key，测完删除）
- `apps/desktop/src-tauri`：
  - 启动时不创建任何窗口（不加载 WebView2），只有托盘；托盘菜单目前只有"退出 LexTrail"
  - 单实例；关闭窗口不退出，只有菜单"退出"会退出
  - 启动时从 `%APPDATA%\io.github.aspeternity.lextrail\settings.toml` 读设置（第一次启动会创建）；读取出问题时弹原生提示框，并用默认设置继续运行
  - 全局快捷键从设置读取（默认 Ctrl+Alt+X，D014）；注册失败时弹原生提示框，托盘提示文字也会改；按下后显示悬浮窗
  - 悬浮窗（`src/popup/`）：
    - 第一次按快捷键时才创建（只有托盘时不加载 WebView2），之后隐藏、复用，不重新加载（D010）
    - 第一次显示等页面加载完成（`PageLoadEvent::Finished`）后再显示，避免出现白屏
    - 位置：默认在光标右下方 12 逻辑像素处，右边或下边放不下时翻到左边或上边，最后限制在光标所在显示器的工作区内（`popup/placement.rs`，纯函数，9 个测试，含负坐标副屏、比工作区还大等情况）
    - 关闭：页面里按 Esc 调用 `hide_popup` 命令；失去焦点（点击外部）时由 Rust 端隐藏
    - 尺寸暂定 380×260 逻辑像素，只有浅色主题，页面是占位内容
  - 提示框统一走 `notice::warn`（`tauri-plugin-dialog`，不加载 WebView）
- `apps/desktop/src`：Vite + Vue，`popup/`、`main/` 两个入口。悬浮窗页面只有 Esc 处理和占位内容（JS 0.6 kB + CSS 0.5 kB，Vue 运行时在公共 chunk 里）；主窗口还是占位页面
- CI（`.github/workflows/ci.yml`）：ubuntu 跑格式、ESLint、类型检查；Windows 跑 clippy、测试，并用 `tauri build --no-bundle` 做 release 构建

本机实测（`tauri build` 的 release 版，粗测，不是正式性能报告；4K 屏 150% 缩放）：
- 只有托盘时：无可见窗口、无子进程，工作集约 13 MB，空闲 CPU ≈ 0
- 悬浮窗冷启动（第一次按快捷键到显示）约 320 ms；之后再显示约 15–20 ms；Esc / 点击外部后约 10–25 ms 隐藏
以上功能都在本机手动验证过；托盘图标已由项目负责人目测确认。

常用命令：
- `pnpm check`：格式、lint、类型检查、全部测试
- `pnpm --filter @lextrail/desktop tauri dev`：开发运行桌面端
- `pnpm --filter @lextrail/desktop tauri build --no-bundle`：构建真正的 release 版。**注意**：直接 `cargo build --release` 得到的仍是开发模式（加载 `devUrl` http://localhost:1420），手动测试和性能测试都必须用 `tauri build`
- `cargo audit`：依赖漏洞检查（本机已装 cargo-audit）

## 正在进行

- 无。

## 下一步

1. M0 第八项：悬浮窗生命周期（隐藏 → `TrySuspend` 挂起 → 快捷键时 `Resume`；收到低内存通知时销毁；"内存优先"模式，对应设置 `popup.mode`）。入口在 `popup::hide` / `popup::show_at_cursor`。
2. 主窗口骨架任务时：托盘菜单加"打开主窗口"，单实例回调唤起主窗口；考虑托盘图标左键直接打开主窗口。
3. M0 期间评审 `ARCHITECTURE.md` 中标注为〔初稿〕的仓库结构部分（第 3 节已加入 `lextrail-config`）。

## 遇到的问题 / 需要项目负责人决定

- 无待决定事项。

已知情况（不需要处理）：
- GitHub Dependabot 有 1 个中等级别提醒，应该是 `glib 0.18.5`（RUSTSEC-2024-0429，unsound）。它只经由 Tauri 的 Linux GTK 托盘依赖引入，**Windows 构建里没有这个 crate**（已用 `cargo tree --target x86_64-pc-windows-msvc` 确认）。Tauri 2 锁定了 gtk 0.18，我们无法单独升级；做 macOS/Linux 支持时再看。可以在 GitHub 上把这条提醒标记为"不影响"（tolerable risk）。`cargo audit` 另有一条 `proc-macro-error` unmaintained 警告，同样只在 Linux 上用到。
- TypeScript 锁定在 `~6.0.3`：TypeScript 7.0 不被 `typescript-eslint` 8.x 支持（peer 要求 `<6.1.0`）。等生态支持 7.x 后再升级。
- 自动化测试注意：用 `Start-Process -WindowStyle Hidden` 启动时，第一个提示框会被系统隐藏（Windows 把启动参数用在进程第一个显示的窗口上）。正常启动不受影响。

## 备注

- 开发环境：项目负责人使用 Windows。M0 的性能测试必须在真实的 Windows 机器上进行。
- 仓库为公开仓库。不要提交任何密钥、Token 或个人数据。
