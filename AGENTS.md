# AGENTS.md — LexTrail AI 协作指南

> 本文件写给参与本项目开发的所有 AI 编程助手（Claude Code、网页版 Claude、Cursor、Codex 等）。
> **每次新对话开始时，你都必须先读完本文件，再按"开始流程"执行。**

---

## 项目简介

LexTrail 是一款常驻后台的 Windows 桌面翻译与词汇学习工具（以后支持 macOS）：在任何软件中选中英文、按快捷键即弹出翻译，结合本地词典与用户自选的 AI 给出语境含义，可收藏到生词本，用 FSRS 复习，并通过自建服务器同步到手机 PWA。

- 技术栈：Tauri 2 + Rust + Vue 3 + TypeScript + SQLite
- 许可证：客户端及共用模块 MIT；`apps/server` AGPL-3.0
- 项目负责人：中文沟通。文档使用中文；代码、标识符、提交说明使用英文。

---

## 必读文件

| 文件 | 内容 | 什么时候读 |
|---|---|---|
| `docs/STATUS.md` | 当前进度、下一步、遇到的问题 | **每次开始时必读** |
| `docs/ROADMAP.md` | 里程碑与任务清单 | **每次开始时必读** |
| `docs/DECISIONS.md` | 所有已确定的决定及理由 | 开始时至少浏览目录；涉及相关模块时细读 |
| `docs/ARCHITECTURE.md` | 架构、目录结构、数据模型、性能预算 | 开发某个模块前读对应章节 |
| `docs/specs/` | 各模块的详细规格 | 开发对应模块时读 |

---

## 铁律

1. **不得推翻已有决定。** `DECISIONS.md` 中的决定未经项目负责人同意不得更改或绕过。你认为某个决定有问题，先说明理由并等待答复。
2. **不得跳出当前里程碑。** 只做 `STATUS.md` 和 `ROADMAP.md` 中当前的任务；不要顺手实现后续里程碑的功能。
3. **不确定时先问。** 尤其是会影响架构、数据格式、对外接口、许可证的问题。
4. **核心逻辑放在 `crates/`，界面层保持很薄**（D006）。
5. **新增同步方式、AI 服务商、平台支持时，使用适配器，不修改上层代码**（D007）。
6. **数据库结构必须带同步元数据**（UUID、`updated_at`、`device_id`、`deleted`），复习日志只追加（D024）。
7. **每个模块都要有自动化测试。** 提交前所有测试必须通过。
8. **不要用"美化数字"的手段应付性能预算**（例如 `EmptyWorkingSet`）。
9. **不要把 API Key、Token 写入代码、日志或配置文件明文。**
10. **crate 和 npm 包名不得使用裸的 `lextrail`**（已被他人占用，D001）。

---

## 开始流程（每次新对话）

1. 读取 `AGENTS.md`（本文件）、`docs/STATUS.md`、`docs/ROADMAP.md`。
2. 浏览 `docs/DECISIONS.md` 的标题；查看最近的 `git log --oneline -15`。
3. 用两三句话向项目负责人复述：
   - 当前处于哪个里程碑；
   - 上次完成了什么；
   - 这次准备做什么。
4. **得到确认后再开始写代码。**

## 结束流程（结束前，或感觉对话快超出长度时）

1. 运行全部测试，确保通过。
2. 更新 `docs/STATUS.md`：完成了什么、正在进行什么（做到哪一步）、下一步、遇到的问题。
3. 在 `docs/ROADMAP.md` 中勾选已完成的任务；新发现的任务追加到对应里程碑并标注 `（新增）`。
4. 如果做出了新的决定，追加到 `docs/DECISIONS.md`（新编号，写明理由），并在 `STATUS.md` 中提一句。
5. 提交并推送代码。
6. 告诉项目负责人：本次做了什么，下次从哪里继续。

> 如果对话可能随时中断，**不要等到最后才更新 `STATUS.md`**。每完成一个任务就更新一次并提交。

---

## 开发规范

### 提交
- 小步提交：每完成一个小任务就提交一次。
- 提交说明使用 Conventional Commits 格式，英文，例如：
  - `feat(capture): add UIA selection reader`
  - `fix(popup): resume webview before show`
  - `docs(status): update after M1 capture work`
- 每个功能在单独的分支上开发，完成后合并到 `main`；`main` 必须始终能构建通过。
- **每次推送后**用 `gh run list --branch <分支> --limit 3` 查看 CI 结果；失败时先修复再做别的事，并在汇报里写明 CI 状态（通过 / 失败 / 还在运行）。**合并到 `main` 前必须确认 CI 通过**（项目负责人的要求）。
- 本地 `pnpm check` 包含 release 配置的 clippy（CI 的 release 构建用 `-D warnings`，只在 debug 版使用的名字在 release 下会报"未使用"）。
- **不要在提交说明和 PR 描述中添加任何 AI 署名**，例如 `Co-Authored-By: Claude …`、`Generated with Claude Code`、会话链接等（项目负责人的要求）。Claude Code 已通过 `.claude/settings.json` 关闭自动署名；其他 AI 工具请手动遵守。
- 提交作者使用项目负责人在本机配置的 Git 身份，不要修改 `user.name` / `user.email`。

### Rust
- `cargo fmt` 格式化；`cargo clippy -- -D warnings` 无警告。
- 错误处理：库代码使用 `thiserror`，应用代码可使用 `anyhow`。
- 平台相关代码用 `#[cfg(target_os = "...")]` 隔离在取词等适配器内部。

### 前端
- TypeScript strict 模式；ESLint + Prettier。
- 悬浮窗页面保持极简：不引入 UI 组件库、路由、状态管理库（D010）。
- 主窗口与 PWA 共用的组件放在 `packages/ui`。

### 测试
- Rust：每个 crate 有单元测试；同步合并逻辑必须有多设备场景的测试。
- 前端：关键组件和流式解析有测试。
- CI 中全部通过才可合并。

---

## 常用命令

> M0 完成后在此补充（安装依赖、开发运行、测试、构建、性能测试）。

```bash
# 待 M0 补充
```
