# 项目进度交接单

> 只写当前状态、下次从哪里开始、未解决的问题；历史以 `git log` 和 `ROADMAP.md` 的勾选为准，长期有效的做法在 `NOTES.md`。

**最后更新**：2026-10-20
**当前阶段**：M1 · 取词与词典（进行中）。取词部分已验收（R1–R4）；词典、查询、词形还原、输入分类已在 `main`。

## 分支

- `feat/popup-dict`（悬浮窗显示词典结果，CI 通过，**未合并**）：等项目负责人做 R5 手动测试。规格 `docs/specs/m1-popup-dict.md`，决定 D046、D044 补充（Edge PDF）。已截图核对 Chrome / Edge PDF 断词、专业释义、Esc、变形词、复合词、查不到、纯中文、句子、滚动；性能（m0-benchmark 7.3）挂起 82.6 MiB、冷启动 301–376 ms，达标。
- `chore/token-rules`（从 `feat/popup-dict` 开出）：重整文档（新建 `NOTES.md`，压缩 `STATUS.md`、`AGENTS.md`），`request: ...` 日志默认只打印内容种类（`LEXTRAIL_DEBUG_LOG_CONTENT=1` 打印完整内容）。**在 `feat/popup-dict` 合并之后再合并。**

## 下次从哪里开始

1. 项目负责人做 R5（`m1-capture-manual-tests.md`"悬浮窗显示词典结果（R5）"，R5-1 至 R5-18）。通过后确认 CI，把 `feat/popup-dict` 合并到 `main`；再把 `main` 合进 `chore/token-rules`、确认 CI 后合并。
2. 之后继续 M1：查询历史接入桌面端 → 连按两次 Ctrl+C（实现 `capture_context`，完成后把这个用法加回提示文字，R1-9）→ 快捷键设置界面。
3. 查询历史接入的要求（项目负责人）：第一次查询时用 `app_local_data_dir()` 按需打开 `Store`；`Store` 放进 `Mutex`，在后台线程读写；打开失败时用 `notice::warn` 提示，之后不记录历史、照常查词；加"不记录查询历史"开关。`kind` 用 `Dictionary::classify` 的结果，`lemma` 用 `LookupResult::lemma()`。

## 未解决的问题

- **B3 没有结论**：VS Code 编辑器里 UIA 报告"没有选中"的原因未确认。`editor.accessibilitySupport` 设为 `on` 后模拟按键做不出选区，测试不成立。剪贴板兜底已覆盖 VS Code，暂不继续；需要时在 `on` 模式下手动选词，用 `capture_probe` 看 UIA 是否取到。
- **获取焦点的后备路径未在真实环境中触发过**（R2-5）：`AttachThreadInput` 重试只经代码审查；"被拒后不带焦点显示"只用模拟开关验证过。日志里看到 `Attached` 或 `Refused` 时记下场景。
- 第一次显示的悬浮窗比之后高（debug 版、1.5 倍缩放：592×446，之后 592×403），原因未查（ROADMAP 已记）。
- 冷启动波动较大（预算 400 ms；历次 281–390 ms，几乎全部是窗口创建和页面加载），继续观察。
- 前端测试框架（Vitest）还没引入（M1 任务）。
- 低优先级：Windows Terminal 跨行选中时句子带上提示符（R1-10）；句首省略号、过长上下文截取（R1-11）；冷启动后 0–100 ms 内按 Esc 可能无效（`m0-benchmark.md` 4.5 节）。
- 推迟到 M4：服务端许可证写法、`apps/server` 是否加入 Cargo workspace（`DECISIONS.md` 待确认事项）。
