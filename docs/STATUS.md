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

## 正在进行

- 无。

## 下一步

1. M0 第四项：Tauri 2 桌面应用骨架（托盘图标、托盘菜单、开机不显示窗口）。
2. Tauri 骨架任务时：把 `apps/desktop/src-tauri` 加入 Cargo workspace 成员；加入 `vue-tsc` 做 `.vue` 文件的类型检查，在根目录加 `typecheck` 脚本，并加进 CI；CI 的 Windows 任务需要先装 Node/pnpm 才能构建前端。
3. M0 期间评审 `ARCHITECTURE.md` 中标注为〔初稿〕的仓库结构部分。

## 遇到的问题 / 需要项目负责人决定

- TypeScript 锁定在 `~6.0.3`：最新的 TypeScript 7.0 不被 `typescript-eslint` 8.x 支持（peer 要求 `<6.1.0`）。等生态支持 7.x 后再升级。

## 备注

- 开发环境：项目负责人使用 Windows。M0 的性能测试必须在真实的 Windows 机器上进行。
- 仓库为公开仓库。不要提交任何密钥、Token 或个人数据。
