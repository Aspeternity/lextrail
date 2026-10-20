# AGENTS.md — LexTrail AI 协作指南

LexTrail：Windows 桌面翻译与词汇学习工具（Tauri 2 + Rust + Vue 3 + SQLite）。客户端 MIT，`apps/server` AGPL-3.0。与项目负责人中文沟通；文档中文，代码和提交说明英文。

## 铁律

1. 不得推翻或绕过 `docs/DECISIONS.md` 的决定；有异议先说理由、等答复。
2. 只做当前里程碑的任务。
3. 不确定（架构、数据格式、接口、许可证）先问。
4. 核心逻辑放 `crates/`，界面层保持很薄（D006）；扩展用适配器（D007）。
5. 数据库带同步元数据，复习日志只追加（D024）。
6. 每个模块有自动化测试，提交前全部通过。
7. 不用"美化数字"的手段应付性能预算。
8. API Key、Token 不进代码、日志、配置明文。
9. crate / npm 包名不用裸的 `lextrail`（D001）。

## 开始流程

1. 读 `docs/STATUS.md` 全文；ROADMAP 只读当前里程碑一节（`grep -n "^## " docs/ROADMAP.md` 定位后按行号读）；`grep -n "^### D" docs/DECISIONS.md` 浏览决定标题。
2. `git log --oneline -15`、`git status`。STATUS 与 git 不一致时以 git 为准，并告诉项目负责人哪里不一致。
3. 项目负责人第一句话说明了任务就直接开始；否则用两三句话复述（里程碑、上次完成、这次要做），等确认。

## 节省 token

- 一个任务一个会话：任务完成、STATUS 更新后提醒开新会话；中断超过约 1 小时也建议开新会话。
- 命令输出用 grep / tail / head 过滤，验证只输出结论（PASS / FAIL 和关键几行）；bench 只取摘要行。
- 不重复读没变的文件；大文件先 `grep -n` 再读片段。DECISIONS 按编号或关键词 grep 后读条目；ARCHITECTURE、specs 只读相关章节，手动测试清单只读当前轮次。
- 能用 `popup-e2e.ps1`（UIA 读文字）核对就不截图；布局、滚动等才截图，只截悬浮窗区域并缩小。
- 长时间等待（bench、CI、构建）放后台，只取结果。
- 文档和提交说明简洁，不重复别处已有内容。

## 做任务

- 一次只做一个任务；发现需要的前置工作先告诉项目负责人。
- 提新方案前 grep `DECISIONS.md`，不重提已否决的方案。
- 改设计须经项目负责人同意，同意后在同一次提交里更新 ARCHITECTURE 或 specs，并在 DECISIONS 追加记录（新编号，写理由）。
- 完成的标准是 ROADMAP 的验收标准逐条通过。

## 结束流程

1. 运行全部测试（`pnpm check`）。
2. 更新 ROADMAP（勾选；新任务标"（新增）"）；更新 STATUS 并删掉过时内容；新的约定和测试方法写进 `docs/NOTES.md`。
3. 代码和文档同一次提交；推送后查 CI；告诉项目负责人做了什么、下次从哪里继续。可能中断时每完成一步就更新 STATUS 并提交。

## 开发规范

- Conventional Commits，英文，小步提交。
- 每个功能单独分支，`main` 始终能构建。推送后 `gh run list --branch <分支> --limit 3` 查 CI，失败先修，汇报写明 CI 状态；**合并到 `main` 前必须确认 CI 通过**。
- **提交说明和 PR 描述不加任何 AI 署名**（`Co-Authored-By`、`Generated with …`、会话链接）。不改 Git 的 `user.name` / `user.email`。
- Rust：库用 `thiserror`、应用可用 `anyhow`；平台代码用 `#[cfg(target_os)]` 隔离在适配器内。前端：悬浮窗不引入 UI 库、路由、状态管理库（D010）；共用组件放 `packages/ui`。同步合并逻辑要有多设备场景测试。

常用命令、debug 开关、编码规范细节、测试方法与坑见 `docs/NOTES.md`。
