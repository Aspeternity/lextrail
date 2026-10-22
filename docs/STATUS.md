# 项目进度交接单

> 每次开发结束前更新本文件（见 `AGENTS.md` 的"结束流程"）。
> **只保留当前状态，控制在 100 行以内。** 已完成任务的过程记录写进对应的 `docs/specs/` 文件，完成情况以 `ROADMAP.md` 的勾选和 `git log` 为准；命令、debug 开关、测试规则见 `DEVELOPMENT.md`。

**最后更新**：2026-10-23（M1.5 第 5 步开头：大小写、前后缀规则待决定两处）
**当前阶段**：M1.5 · 牛津词典（第 2 步转换工具完成，下一步扩展词典读取与查询合并）

---

## 当前状态

M0 全部完成（路线 A，D036）。**M1 完成**（2026-10-21；剩下的 5 个低优先级项移到 M6）：

- 取词：UIA（D041）+ 剪贴板兜底（D043）+ 悬浮窗焦点方案 B（D042）+ 快捷键屏蔽键与防卡键（D014 补充）。手动测试 R1–R4 验收。
- 词典：`tools/dict-builder`、`lextrail-dict`、悬浮窗词典结果（D044–D046），R5 通过。
- 查询历史写入本地数据库（D039、D040）。
- 连按两次 Ctrl+C（D012，默认关闭，`[hotkeys] double_ctrl_c`），R6、R7 验收（`specs/m1-double-copy.md`）。
- 悬浮窗高度随内容调整（D046 补充，`specs/m1-popup-dict.md` 第八节），R8 验收。
- 设置窗口与快捷键设置（D047、D048，`specs/m1-hotkey-settings.md`）：托盘"设置…"；默认快捷键 Alt+Space（D014）；只禁止不能用的组合，可以不使用快捷键；只有 Alt 的组合补发 Ctrl；连按两次 Ctrl+C、不记录查询历史两个开关即时生效。R9、R10 验收。

- 前端测试：Vitest + jsdom，Rust 命令用 `@tauri-apps/api/mocks` 代替；覆盖悬浮窗页面、词典结果、快捷键录入、设置开关（64 个），`pnpm test` 和 CI 都会跑（`DEVELOPMENT.md` 第 1 节）。

手动测试清单与各轮结果：`specs/m1-capture-manual-tests.md`。
性能：最近一次 `specs/m0-benchmark.md` 7.7（2026-10-21，默认 Alt+Space）：托盘 1.6 MiB、挂起 85.7 MiB、设置窗口关掉后挂起 43.8 MiB、冷启动 289.0 ms、热显示 37.5 ms、挂起后恢复 34.8 ms，全部达标；热显示比 7.5 慢约 10 ms，待观察。

## 正在进行

- M1.5 · 牛津词典（D049，新增里程碑）：第 0 步调研和补充数据在 `specs/m1.5-oxford.md` 第 0 节（只有结构和统计）；spec 正文已批准（2026-10-22）：编号方案 B（D052）、发音方案 C（D053）、默认中英释义 + 开关（D054）。第 1、2 步完成（分支 `feat/oxford`）。
  - 已定：读取库 `mdictlib =0.2.8`（D050）；扩展词典位置与设置（D051）；数据库先记下全部音频文件名。
  - 源文件已移出仓库，在 `D:\LexTrailData\oxford-source\HKOUP_mdict_export20260925\`（转换工具用命令行参数指定位置）；项目负责人自己的转换结果放 `D:\LexTrailData\dicts\`。
  - 调研临时文件夹 `D:\Projects\lextrail-oxford-scratch` 已在 spec 批准后删除。

## 下一步

1. 第 2 步转换工具 `tools/oxford-builder` 已完成（spec 3.1）：真实转换约 14 s，`oalecd10.db` 51.4 MiB、`oalecd10-audio.db` 429.5 MiB；第 0 节的计数全部对上；只有 `other` 关系的键 1,488 个（1.2%）；繁体版对照 117,992 个编号全部不变。项目负责人自己的转换结果在 `D:\LexTrailData\dicts\`。
   - 第 2 步补充（项目负责人 2026-10-22）：只有 `other` 关系的键按"牛津查不到"走 ECDICT 流程，1,488 个键的去向见 spec 6.3 末（只剩 Related 760）；新旧编号对照加 `changed`（编号还在、内容改过），spec 5.4；两个输出文件都写完后才一起替换；`dump` 示例拒绝输出到仓库内。
   - 第 1 步补充（项目负责人 2026-10-22）：音标规则判断词尾 / 元音前时跳过重音符号（`riˈækt`）；ECDICT 补上的词条的关系说明只用牛津的 inflection / variant / derivative（6.2）；音标一致率重算见 spec 第 8 节（61.9%，不计重音 72.2%）。
2. 第 3 步完成（2026-10-22）：扩展词典读取、查询合并、排序（`lextrail-dict` 的 `ext::reader`、`Lexicon`，spec 5.4、6.1–6.3）；真实数据查词 0.1–1.4 ms（`examples/merged.rs`）。`D:\LexTrailData\dicts\` 已按最终规则重新转换。第 5 步前要先把 `better` 的关系合并写法、变体词条上的说明写进 spec 6.3 给项目负责人看。
3. 第 4 步设置与状态完成（spec 2.2、2.3）：`[dictionaries]`（folder / oxford / english_definitions）；设置窗口"词典"分类（文件夹、选择文件夹…、恢复默认、状态、两个开关），改了立即生效（关闭已打开的连接）；`ExtDictionaryService` 已就绪，悬浮窗第 5 步接入。`settings-e2e.ps1` 加了词典检查（UIA，全部通过）；"选择文件夹…"的对话框只能手动测（第 12 节第 2 条）。
   - 第 4 步补充（项目负责人 2026-10-22）："转换于"按本机时区显示日期（`local_date.rs`，测试显式传 UTC+8 时区）；默认文件夹不存在时显示"没有找到牛津词典"，程序不建这个文件夹（spec 2.3）。
4. **正在做**：第 5 步开头（项目负责人 2026-10-23 批准 6.3 合并写法和变体说明，另加大小写、前后缀两条规则）。已实现（查询时，`merge.rs`）：关系文字合并（`Link`、`merge_links`）、大小写 a/b/c（`by_case`、`Role::OtherCase`）、前后缀词条只在没有其他结果时显示（`is_affix`）。全键对比统计：大小写影响 890 个键、前后缀 687 个、只改关系文字 3,825 个。**等项目负责人决定两处后再写进 6.3**：ECDICT 补充词条在牛津词条被移到后面时也加进来（`aids` 出现"AIDS（aid 的复数）"）；句中首字母大写的常用词碰上元素符号等完全一致的键（`In` → indium，in 移到最后）。另 `Bi`/`Ni`/`Ti` 等 9 行是 ECDICT `forms` 的错误数据（"bi 是 BI 的复数"）带出的自身变形，要改转换工具重新转换。
5. 之后：第 5 步悬浮窗显示（含变体词条说明"area 义项 6 所指"）。
6. 之后：发音播放、性能和手动测试。

## 待跟进（不在 ROADMAP 中的观察项）

- **三个模拟按键的 e2e 脚本还没实际跑过清理逻辑**：`popup-e2e.ps1`、`hotkey-classes-e2e.ps1`、`stuck-keys.ps1` 结束时删除本次临时文件夹（`run-dirs.ps1`）只做了语法检查；下次正式跑时确认 `%TEMP%` 下没有留下 `lextrail-e2e-edge-*`、`lextrail-e2e-print-*` 和 `-OutDir` 下的 `config-*`、`data-*`、`vscode-*`。（`settings-e2e.ps1` 已实测）
- **B3 没有结论**：VS Code 编辑器里 UIA 报告"没有选中"的原因未确认；剪贴板兜底已覆盖，暂不继续。如需要，可在 `editor.accessibilitySupport = on` 下手动选词，用 `capture_probe` 看 UIA 是否取到。
- 获取焦点"被拒后不带焦点显示"仍只用模拟开关验证过。
- 冷启动后 0–100 ms 内按 Esc 可能无效（`m0-benchmark.md` 4.5 节），低优先级。
- `settings-e2e.ps1` 第一次运行（WebView2 冷启动）时 "double Ctrl+C" 两条偶发失败，紧接着连跑两次全过；再出现时查 `Toggle` 是否在页面就绪前点击。
- 冷启动波动较大（预算 400 ms，历次 264–376 ms），几乎全部是窗口创建和页面加载，继续观察。

## 遇到的问题 / 需要项目负责人决定

- C 盘空间：2026-10-22 清理了以前测试留下的临时文件夹（242 个，约 2.9 GiB），C 盘剩约 37 GB。测试脚本结束时自动删除本次的临时文件夹（分支 `fix/e2e-temp-cleanup`）。

- 服务端许可证写法和 `apps/server` 是否加入 Cargo workspace 推迟到 M4（见 `DECISIONS.md` 待确认事项）。

## 备注

- 开发环境：项目负责人使用 Windows。性能测试和手动测试必须在真实的 Windows 机器上进行。
- 仓库为公开仓库。不要提交任何密钥、Token 或个人数据。
