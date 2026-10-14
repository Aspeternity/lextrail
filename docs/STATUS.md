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

## 正在进行

- 无。

## 下一步

1. M0 第二项：代码规范（`rustfmt`、`clippy`、TypeScript strict、ESLint、Prettier）。
2. Tauri 骨架任务时把 `apps/desktop/src-tauri` 加入 Cargo workspace 成员。
3. M0 期间评审 `ARCHITECTURE.md` 中标注为〔初稿〕的仓库结构部分。

## 遇到的问题 / 需要项目负责人决定

- 无。

## 备注

- 开发环境：项目负责人使用 Windows。M0 的性能测试必须在真实的 Windows 机器上进行。
- 仓库为公开仓库。不要提交任何密钥、Token 或个人数据。
