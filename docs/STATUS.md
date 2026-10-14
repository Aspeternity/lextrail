# 项目进度交接单

> 每次开发结束前更新本文件（见 `AGENTS.md` 的"结束流程"）。
> 只保留**当前**状态；已完成的历史以 `ROADMAP.md` 的勾选和 `git log` 为准。

**最后更新**：2026-10-15
**当前阶段**：M0 · 项目骨架与性能原型

---

## 当前状态

规划阶段已完成，项目负责人已审核通过。项目的定位、技术栈、架构和第一版范围记录在：
- `docs/DECISIONS.md`（D001–D034、暂缓事项）
- `docs/ARCHITECTURE.md`
- `docs/ROADMAP.md`

GitHub 公开仓库已创建，规划文档已推送。

M0 第一项"初始化 monorepo"已完成：
- 根目录 `Cargo.toml`（workspace，成员 `crates/*`，edition 2024，统一 `workspace.package` 元数据，`publish = false`）
- 根目录 `package.json` + `pnpm-workspace.yaml`（成员 `apps/desktop`、`apps/pwa`、`packages/*`，目前都还没有 `package.json`）
- `crates/` 下四个空 crate：`lextrail-core`、`lextrail-dict`、`lextrail-ai`、`lextrail-capture`（只有模块文档注释，尚无代码和依赖）
- `apps/desktop`、`apps/server`、`apps/pwa`、`packages/ui`、`tools/dict-builder`、`tools/bench`、`docs/specs` 目前只放了 README 占位，说明用途和计划实现的里程碑
- 目录结构完全按 `ARCHITECTURE.md` 第 3 节初稿，未作改动

M0 第二项"代码规范"已完成：
- Rust：`rustfmt.toml`（仅稳定选项）、`rust-toolchain.toml`（stable + rustfmt/clippy）、`Cargo.toml` 中的 `[workspace.lints]`（各 crate 通过 `[lints] workspace = true` 继承）
- TypeScript：`tsconfig.base.json`（`strict` + `noUncheckedIndexedAccess`、`exactOptionalPropertyTypes` 等），各前端包以后 `extends` 它
- ESLint：`eslint.config.mjs`（flat config：`@eslint/js` + `typescript-eslint` strict + `eslint-plugin-vue` recommended + `eslint-config-prettier`）
- Prettier：`.prettierrc.json`；`.prettierignore` 排除了所有 `*.md`（中文文档和宽表格不让 Prettier 重排）
- `.editorconfig`、`.vscode/extensions.json`
- 根目录脚本：`pnpm lint`、`pnpm format`、`pnpm format:check`、`pnpm test`、`pnpm check`（全部检查）
- 已用临时文件验证 ESLint（TS 和 Vue）、`tsc` strict、Prettier 都能报出问题

M0 第三项"GitHub Actions"已完成：`.github/workflows/ci.yml`，推送到任何分支、向 `main` 提 PR 时都会运行，同一分支的旧运行会被取消。
- `Format & ESLint`（ubuntu）：`pnpm format:check`（Prettier + `cargo fmt --check`）、ESLint
- `Clippy, test & build (Windows)`（windows-latest）：clippy `-D warnings`、`cargo test`、release 构建，都加了 `--locked`；在 Windows 上跑是为了覆盖 `cfg(target_os = "windows")` 的代码
- 第一次运行全部通过

M0 第四项"Tauri 2 桌面应用骨架"已完成：
- `apps/desktop/src-tauri`（crate `lextrail-desktop`，已加入 Cargo workspace）：Tauri 2.12 + `tray-icon` 特性
  - 启动时**不创建任何窗口**（`app.windows: []`），因此不加载 WebView2，符合"纯托盘状态无 WebView"的预算
  - 托盘图标 + 菜单（目前只有"退出 LexTrail"）；菜单动作用 `TrayAction` 枚举按 id 分发，带单元测试
  - `RunEvent::ExitRequested { code: None }` 时阻止退出：以后关闭/销毁主窗口不会退出程序，只有菜单"退出"会退出
  - 单实例（`tauri-plugin-single-instance`，新增）：第二次启动会立即退出；回调里预留了"唤起主窗口"
  - release 构建使用 `windows_subsystem = "windows"`，没有控制台窗口
  - 图标是临时占位图（蓝底白色"L"），由 `pnpm tauri icon` 生成
  - `bundle.active = false`，安装包留到 M6
- `apps/desktop` 前端：Vite + Vue，两个独立入口 `src/popup/`、`src/main/`（目前只是占位页面），`vue-tsc` 类型检查
- 根目录新增 `pnpm typecheck`，并加入 `pnpm check` 和 CI；CI 的 Windows 任务先构建前端再编译 Rust
- 本机实测（release，粗测，不是正式性能报告）：启动后无可见窗口、无子进程；工作集约 13.6 MB；空闲 CPU ≈ 0；第二个实例 26 ms 内退出

- 应用标识符 `io.github.aspeternity.lextrail` 已经项目负责人确认（2026-10-15）

M0 第五项"全局快捷键注册"已完成（`apps/desktop/src-tauri/src/hotkey.rs`）：
- 使用 `tauri-plugin-global-shortcut`（Windows 上是 `RegisterHotKey`）；`HotkeyAction` 枚举 + `Binding`（动作 → 快捷键字符串）
- 目前只有一个动作"划词翻译"，默认 **Ctrl+Alt+X**（D014 暂定值）；绑定暂时取默认值，下一项"设置"完成后改为从配置文件读取
- 某个快捷键失败不影响其他快捷键注册；失败分"格式无效"和"已被其他程序占用"两类
- 失败提示：原生消息框（`tauri-plugin-dialog`，不加载 WebView）+ 托盘提示文字改为"LexTrail（快捷键注册失败）"；提示内容写明动作、快捷键和原因，并建议关闭占用的程序或在设置中更换
- 按下快捷键目前只在 debug 构建的控制台打印一行；接取词和悬浮窗留到悬浮窗任务
- 单元测试 6 个（默认值可解析且不重复、Ctrl+Alt+X、无效格式、提示文案）
- 本机实测：模拟按下 Ctrl+Alt+X 能触发；先让另一个进程占用 Ctrl+Alt+X 再启动，会弹出警告框；无冲突时 release 工作集约 12.9 MB、无子进程
- 注意：用 `Start-Process -WindowStyle Hidden` 启动时，消息框会被系统隐藏（Windows 把启动参数用在进程第一个显示的窗口上）。正常启动不受影响，只是自动化测试时要注意

常用命令：
- `pnpm check`：格式、lint、类型检查、测试
- `pnpm --filter @lextrail/desktop tauri dev`：开发运行桌面端

## 正在进行

- 无。

## 下一步

1. M0 第六项：设置的读写（本地配置文件），API Key 存入系统凭据管理器；快捷键绑定改为从设置读取。
2. 主窗口骨架任务时：托盘菜单加"打开主窗口"，单实例回调唤起主窗口；考虑托盘图标左键直接打开主窗口。
3. M0 期间评审 `ARCHITECTURE.md` 中标注为〔初稿〕的仓库结构部分。

## 遇到的问题 / 需要项目负责人决定

- 托盘图标和"退出"菜单请你在本机目测确认一次（自动检查只能确认程序存活、托盘创建没有报错）。

- TypeScript 锁定在 `~6.0.3`：最新的 TypeScript 7.0 不被 `typescript-eslint` 8.x 支持（peer 要求 `<6.1.0`）。等生态支持 7.x 后再升级。

## 备注

- 开发环境：项目负责人使用 Windows。M0 的性能测试必须在真实的 Windows 机器上进行。
- 仓库为公开仓库。不要提交任何密钥、Token 或个人数据。
