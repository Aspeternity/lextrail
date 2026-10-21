# 项目进度交接单

> 每次开发结束前更新本文件（见 `AGENTS.md` 的"结束流程"）。
> **只保留当前状态，控制在 100 行以内。** 已完成任务的过程记录写进对应的 `docs/specs/` 文件，完成情况以 `ROADMAP.md` 的勾选和 `git log` 为准；命令、debug 开关、测试规则见 `DEVELOPMENT.md`。

**最后更新**：2026-10-21（R9 通过；按项目负责人要求简化快捷键规则、修好只有 Alt 的组合、加"不使用快捷键"）
**当前阶段**：M1 · 取词与词典（进行中）

---

## 当前状态

M0 全部完成（路线 A，D036）。M1 已完成并合并到 `main`：

- 取词：UIA（D041）+ 剪贴板兜底（D043）+ 悬浮窗焦点方案 B（D042）+ 快捷键屏蔽键与防卡键（D014 补充）。手动测试 R1–R4 验收。
- 词典：`tools/dict-builder`、`lextrail-dict`、悬浮窗词典结果（D044–D046），R5 通过。
- 查询历史写入本地数据库（D039、D040）；"不记录查询历史"目前只能改设置文件 `[history] record = false`。
- 连按两次 Ctrl+C（D012，默认关闭，`[hotkeys] double_ctrl_c`），R6、R7 验收（`specs/m1-double-copy.md`）。
- 悬浮窗高度随内容调整（D046 补充，`specs/m1-popup-dict.md` 第八节），R8 验收。

手动测试清单与各轮结果：`specs/m1-capture-manual-tests.md`。
性能：最近一次 `specs/m0-benchmark.md` 7.7（2026-10-21，默认 Alt+Space）：托盘 1.6 MiB、挂起 85.7 MiB、设置窗口关掉后挂起 43.8 MiB、冷启动 289.0 ms、热显示 37.5 ms、挂起后恢复 34.8 ms，全部达标；热显示比 7.5 慢约 10 ms，待观察。

## 正在进行

- **设置窗口与快捷键设置**（分支 `feat/hotkey-settings`，spec `docs/specs/m1-hotkey-settings.md`）：
  - 代码、模拟按键测试、bench 7.6、R9 真实键盘验收（17 项）都已完成。
  - R9 后项目负责人要求重新考虑快捷键规则，已做完（自动检查全部通过）：
    - **D048**：只禁止不能用的组合（LexTrail 自己用的、Windows 保留的、不带 Ctrl / Alt 的、带 Win 的），其他一律允许，不提示也不确认；删除软件快捷键清单和"提示但允许"。
    - **不使用快捷键**：设置窗口"不使用"，设置文件 `lookup = ""`。
    - **D014 补充**：只有 Alt 时屏蔽键改为补发一次 Ctrl，Windows Terminal 里也能取词；开了"按 Ctrl 键时显示指针位置"时改为取词后再补发 `0xE8`。
    - **默认快捷键改为 Alt+Space**（D014 ✅）。
    - bench 7.7（Alt+Space，bench 改为用临时设置目录）全部达标。
  - 剩下：R10 真实键盘复测（`m1-capture-manual-tests.md`）；合并到 `main`。

## 下一步

1. R10 真实键盘复测（AI 启动程序、读日志）；通过后合并到 `main`。
2. 前端测试框架（Vitest）还没引入（M1 任务）。

## 待跟进（不在 ROADMAP 中的观察项）

- **B3 没有结论**：VS Code 编辑器里 UIA 报告"没有选中"的原因未确认；剪贴板兜底已覆盖，暂不继续。如需要，可在 `editor.accessibilitySupport = on` 下手动选词，用 `capture_probe` 看 UIA 是否取到。
- 获取焦点"被拒后不带焦点显示"仍只用模拟开关验证过。
- 冷启动后 0–100 ms 内按 Esc 可能无效（`m0-benchmark.md` 4.5 节），低优先级。
- 冷启动波动较大（预算 400 ms，历次 264–376 ms），几乎全部是窗口创建和页面加载，继续观察。

## 遇到的问题 / 需要项目负责人决定

- 服务端许可证写法和 `apps/server` 是否加入 Cargo workspace 推迟到 M4（见 `DECISIONS.md` 待确认事项）。

## 备注

- 开发环境：项目负责人使用 Windows。性能测试和手动测试必须在真实的 Windows 机器上进行。
- 仓库为公开仓库。不要提交任何密钥、Token 或个人数据。
