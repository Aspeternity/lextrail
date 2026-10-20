# 项目进度交接单

> 每次开发结束前更新本文件（见 `AGENTS.md` 的"结束流程"）。
> **只保留当前状态，控制在 100 行以内。** 已完成任务的过程记录写进对应的 `docs/specs/` 文件，完成情况以 `ROADMAP.md` 的勾选和 `git log` 为准；命令、debug 开关、测试规则见 `DEVELOPMENT.md`。

**最后更新**：2026-10-21（设置窗口代码完成，模拟按键测试待运行）
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
性能：最近一次 `specs/m0-benchmark.md` 7.5（2026-10-21）：托盘 1.6 MiB、挂起 83.3 MiB、冷启动 269.6 ms、热显示 25.1 ms、挂起后恢复 24.1 ms，全部达标。

## 正在进行

- **设置窗口与快捷键设置**（分支 `feat/hotkey-settings`，spec `docs/specs/m1-hotkey-settings.md`，已审阅）：
  - 代码完成：`lextrail-config::hotkey`（解析、规则、已核对的冲突表）、设置文件手改检查；桌面端 `hotkey.rs` 状态机、`settings.rs`、`settings_window.rs`、托盘"设置…"、`--settings`、每个窗口只能调用自己的命令；前端 `src/settings/`。单元测试和 `pnpm check` 通过，CI 通过。
  - 快捷键清单已核对（spec 第七节），建议保留 Ctrl+Alt+X，**待项目负责人在本机确认 Acrobat、福昕阅读器、国产软件后决定 D014**。
  - `settings-e2e.ps1` 不发按键的部分全部通过。已写好、**还没运行**（需要项目负责人同意模拟按键）：`settings-e2e.ps1 -Keys`、`hotkey-classes-e2e.ps1`（五类修饰键：Windows Terminal 选词、VS Code 菜单栏、stuck-keys）、`popup-e2e.ps1 -Hotkey`。bench 加了"设置窗口打开又关闭后"，还没跑。
  - 剩下：上述模拟按键测试、bench 重测（数据先交项目负责人）、手动测试清单 R9、ROADMAP 勾选。

## 下一步

1. 项目负责人同意后运行模拟按键测试和 bench；修复发现的问题。
2. 写 R9 手动测试清单，项目负责人真实键盘验收后合并到 `main`。
3. 前端测试框架（Vitest）还没引入（M1 任务）。

## 待跟进（不在 ROADMAP 中的观察项）

- **B3 没有结论**：VS Code 编辑器里 UIA 报告"没有选中"的原因未确认；剪贴板兜底已覆盖，暂不继续。如需要，可在 `editor.accessibilitySupport = on` 下手动选词，用 `capture_probe` 看 UIA 是否取到。
- 获取焦点"被拒后不带焦点显示"仍只用模拟开关验证过。
- 冷启动后 0–100 ms 内按 Esc 可能无效（`m0-benchmark.md` 4.5 节），低优先级。
- 冷启动波动较大（预算 400 ms，历次 264–376 ms），几乎全部是窗口创建和页面加载，继续观察。

## 遇到的问题 / 需要项目负责人决定

- 默认快捷键 D014 待决定（spec 第七节 7.3）；模拟按键测试待同意。服务端许可证写法和 `apps/server` 是否加入 Cargo workspace 推迟到 M4（见 `DECISIONS.md` 待确认事项）。

## 备注

- 开发环境：项目负责人使用 Windows。性能测试和手动测试必须在真实的 Windows 机器上进行。
- 仓库为公开仓库。不要提交任何密钥、Token 或个人数据。
