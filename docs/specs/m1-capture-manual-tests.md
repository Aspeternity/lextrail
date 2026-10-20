# M1 取词手动测试清单

> 自动化测试覆盖不到真实程序的 UI Automation 行为，每次修改取词或悬浮窗焦点逻辑后按本清单手动测一遍。
> 结果填在每个表格的"结果"列（✅ / ❌ / 说明），并在末尾写明日期、Windows 版本和各软件版本。
> 规格见 `ARCHITECTURE.md` 4.1、4.2、9.3，决定见 D041、D042、D043。
>
> **改动快捷键或键盘相关逻辑之后**（`hotkey.rs`、`keyboard.rs` 屏蔽键、剪贴板兜底的模拟按键等），除了 R2-1、R2-2（VS Code、Word、记事本、Edge 不激活菜单栏 / KeyTips），**必须复测 Windows Terminal 选词**（5.1：取到单词、选区保留）。R3-9 中屏蔽键就是这样在 Windows Terminal 中引入了回归，而 R2 没有测到。

## 0. 准备

- 测试前先退出正在运行的 LexTrail（托盘菜单"退出"），否则单实例会让新启动的程序直接退出。
- **debug 版**（带日志，手动测试用这个）：

  ```bash
  pnpm --filter @lextrail/desktop tauri build --debug --no-bundle
  ```

  生成 `target\debug\lextrail-desktop.exe`。在终端里运行它，控制台会打印日志。每次取词打印一行（不含取到的文字）：

  ```text
  capture #2: captured | uia=captured 3.1ms (uia_focus); total 3.1ms
  ```

  依次是：结果类型（`captured` / `no_selection` / `elevated` / `no_foreground_window` / `unsupported` / `timed_out` / `error`）、每一步的取词方式、结果和耗时、焦点元素来源（`uia_focus` = UIA 焦点元素，`hwnd_focus` / `hwnd_caret` = `GetGUIThreadInfo` 的焦点 / 光标窗口，`foreground_window` = 前台窗口），最后是总耗时。前置检查直接决定结果时显示为 `precheck=elevated; total 0.0ms`。另外还有 `request: ...`（悬浮窗显示与焦点）和 `popup: ...`（窗口生命周期）两类日志。
- **release 版**（与发布一致，测性能、看最终效果用这个；没有控制台日志）：

  ```bash
  pnpm --filter @lextrail/desktop tauri build --no-bundle
  ```

  生成 `target\release\lextrail-desktop.exe`。注意不要用 `cargo build --release`，那样得到的程序会去加载开发服务器的页面。
- **悬浮窗内容的自动检查**（AI 做端到端验证时必须用它，或者截图亲眼看到内容；只看日志不算）：

  ```bash
  powershell -NoProfile -ExecutionPolicy Bypass -File tools/e2e/popup-e2e.ps1 -App target/debug/lextrail-desktop.exe
  ```

  加 `-Pdf` 时改为把测试页打印成 PDF、在 Edge 的 PDF 阅读器中打开：此时不用 UIA、由剪贴板兜底取词，用来检查模拟 Ctrl+C 之后悬浮窗仍能获得焦点（没有句子行）。再加 `-HoldMs 2000` 时按下快捷键后 Ctrl+Alt 按住 2 秒，检查按住期间显示"松开快捷键后取词…"、松手后取到单词；`-HoldMs 3600` 检查显示"请松开快捷键后再试"；`-HoldMs 200` 相当于正常按一下。

  它会启动应用和一个全新的 Edge（InPrivate，独立配置），用真实鼠标双击 `tools/e2e/list-item.html` 中的单词、按真实快捷键，然后检查悬浮窗可见、有焦点，并通过 UI Automation 读出悬浮窗里的文字，核对单词和句子（不含列表符号）。每次查词保存一张截图（`%TEMP%\lextrail-e2e\`）。运行期间会移动鼠标、模拟按键，不要操作电脑。失败时退出码为 1。
- **屏蔽键会不会让修饰键卡住**（D014 补充）：`tools/e2e/stuck-keys.ps1` 打开一个自己的小窗口，用模拟按键按很多次 Ctrl+Alt+X，并让松开按键与补发的 Ctrl 竞争（间隔 0 ms 起），每次检查没有修饰键卡住：

  ```bash
  powershell -NoProfile -ExecutionPolicy Bypass -Command "& ./tools/e2e/stuck-keys.ps1 -App target/debug/lextrail-desktop.exe -GapsMs 0,0,1,2,3 -Rounds 24"
  ```

  debug 版每次补发打印一行，如 `hotkey: alt mask: repeat LeftCtrl (0.5ms; hook saw A2v*)`（`A2` = 左 Ctrl，`v` 按下，`^` 松开，`*` 我们补发的）；出现竞争并已补发松开时为 `released first, key-up sent`。
- 只测某个窗口能不能取到词时，可以用探针，不必启动整个应用：

  ```bash
  cargo run -p lextrail-capture --example capture_probe -- 3
  ```

  运行后 3 秒内切换到目标窗口并选中文字，探针会打印窗口信息、取到的文字、段落、句子和耗时。
- 只想测剪贴板兜底时（例如在 UIA 能取到词的 Windows Terminal 里测 Ctrl+Insert），设 `LEXTRAIL_DEBUG_CLIPBOARD_ONLY=1`（只在 debug 构建中有效）跳过 UIA。
- 测"正在取词"和切换窗口的情况时，设置环境变量 `LEXTRAIL_DEBUG_CAPTURE_DELAY_MS=800`（只在 debug 构建中有效），人为让取词变慢。

说明：选中的内容和整个上下文相同时（例如在记事本中选中整行），悬浮窗不显示句子行，避免重复（`popup/content.rs`，有单元测试），这是有意的设计。

每个软件都检查以下几点：
- **文字**：取到的文字与选中的一致。
- **句子**：悬浮窗显示的句子是选中文字所在的那一句（不是整段，也没有截断）。
- **焦点**：取词期间原窗口保持焦点（光标、选区不变）；悬浮窗出现后按 Esc 能关闭，焦点回到原窗口。
- **剪贴板**：取词前后剪贴板内容不变（UIA 不应碰剪贴板）。
- **耗时**：日志或探针显示的取词时间；超过 180 ms 时记下来。

## 1. 浏览器

| # | 软件 | 操作 | 预期 | 结果 |
|---|---|---|---|---|
| 1.1 | Edge | 普通网页正文中双击选中一个单词，按快捷键 | 取到单词和所在句子 | ✅ Edge：addresses / house，句子正确；Esc 后选区保持；剪贴板未被改动（`uia=captured 13.0ms`、`6.4ms`） |
| 1.2 | Edge | 拖动选中跨两句的一段文字 | 取到整段选区；句子为这两句 | ✅ 选区和两句完整（6.7 ms） |
| 1.3 | Edge | 在 `<input>` 输入框和 `<textarea>` 中选中文字 | 取到输入框中的选区 | ✅ 搜索框取到 brown，句子为框内文字；源代码大文本框能取到，但上下文是一整行很长的代码（见问题 R1-11） |
| 1.4 | Chrome | 重复 1.1–1.3 | 同上 | ✅ Chrome 冷启动第一次 `x2` 42.0 ms 取到；之后 1.1–1.3 均正确（3.8–5.3 ms）。维基百科"你知道吗"的句子开头带 "..."（见问题 R1-11） |
| 1.5 | Firefox | 重复 1.1–1.3 | 同上；如取不到，记下 `Unsupported` 还是 `NoSelection` | 未测（Firefox 未安装） |
| 1.6 | 任一浏览器 | 什么都不选，按快捷键 | 显示"没有选中文字" | ✅ 显示"没有选中文字"（4.1 ms） |

## 2. PDF 阅读器

| # | 软件 | 操作 | 预期 | 结果 |
|---|---|---|---|---|
| 2.1 | Edge 内置 PDF | 选中一个单词 | 剪贴板兜底完成前：显示"这个窗口暂时无法取词"，日志为 `uia=unsupported ... [chromium_pdf]`（Edge 的 PDF 阅读器报告的选区会错位，故不用 UIA，见 ARCHITECTURE 4.2）。剪贴板兜底完成后：取到单词；记下句子是否完整（PDF 常有硬换行） | ✅（按新规则）Edge 本地 PDF 显示"暂时无法取词"，`unsupported 55.7ms x2 [chromium_pdf]`。Chrome PDF 未被识别为 `[chromium_pdf]`：刚打开时 `no_selection 48.6ms x2`（实际已选中），约 10 秒后取到 batching；网址打开的 PDF 取到 improving，但句子里有 `\u{2}`（见问题 R1-6、R1-7） |
| 2.2 | Edge 内置 PDF | 选中跨行的短语 | 同 2.1；剪贴板兜底完成后：取到短语，换行合并为空格 | 未单独测 |
| 2.3 | Adobe Acrobat Reader | 重复 2.1–2.2 | 同上；如取不到，记下结果类型 | 未测 |
| 2.4 | SumatraPDF | 重复 2.1 | 预计取不到（自绘界面），应显示"这个窗口暂时无法取词"，等剪贴板兜底完成后再测 | 未测 |

## 3. Word

| # | 软件 | 操作 | 预期 | 结果 |
|---|---|---|---|---|
| 3.1 | Word | 正文中选中一个单词 | 取到单词和所在句子 | ✅ arrived / Mr. Smith arrived at 3.14 p.m. yesterday.（8.2 ms） |
| 3.2 | Word | 选中含缩写（Mr.、e.g.）的句子中的单词 | 句子不在缩写处被截断 | ✅ hammer（e.g.）、team（U.S.）切句正确 |
| 3.3 | Word | 表格单元格中选中文字 | 取到文字 | ❌ 取到 world，但句子末尾有 `\u{7}`（单元格结束符），显示为方块（见问题 R1-7） |
| 3.4 | Word | 刚打开 Word 后的第一次取词 | 记下耗时；超过 180 ms 时记下来（可能需要调整预算） | ✅ 7.3 ms |
| 3.5 | Word | 按快捷键后检查 Word 的功能区 | 不出现 Alt 键提示（KeyTips） | ✅ 没有 KeyTips；Esc 后选区保持 |

## 4. VS Code

| # | 软件 | 操作 | 预期 | 结果 |
|---|---|---|---|---|
| 4.1 | VS Code | 编辑器中选中一个单词 | 取到单词；记下上下文是行还是段落 | ❌ 已选中（状态栏"已选择 5"），UIA 返回 `no_selection`（14.3 ms；等 10 秒后 8.0 ms）（见问题 R1-3） |
| 4.2 | VS Code | Markdown 预览中选中文字 | 取到文字 | ⚠️ 拖选能取到 sunny / It was a sunny day.（6.5 ms）；但 Esc 后焦点停在菜单栏，"文件(F)"被框住，按 ↓ 弹出菜单，release 版 3/3 复现（见问题 R1-1） |
| 4.3 | VS Code | 集成终端中选中文字 | 记下结果（xterm.js 可能取不到，等剪贴板兜底） | ❌ 已选中，UIA 返回 `no_selection`（8.6 ms）；终端输入行无多余字符；Esc 后菜单栏未被框住（见问题 R1-3） |

## 5. 终端

| # | 软件 | 操作 | 预期 | 结果 |
|---|---|---|---|---|
| 5.1 | Windows Terminal | 输出中用鼠标选中一个单词 | 取到单词；上下文是一行 | ✅ brown / The quick brown fox jumps over the lazy dog.（8.0 ms） |
| 5.2 | Windows Terminal | 选中跨行文字 | 取到选中文字 | ✅ 取到跨行文字，但句子带上了提示符 `PS C:\Users\Asp>`（见问题 R1-10） |
| 5.3 | 旧版控制台（conhost） | 重复 5.1 | 记下结果 | 预期内：`unsupported 13.2ms`，显示"暂时无法取词"，等剪贴板兜底 |
| 5.4 | 任一终端 | 什么都不选，按快捷键 | 显示"没有选中文字"；终端中不出现多余字符 | ✅ 显示"没有选中文字"（5.8 ms） |

## 6. 特殊情况

| # | 场景 | 操作 | 预期 | 结果 |
|---|---|---|---|---|
| 6.1 | 管理员窗口 | 以管理员身份运行记事本，选中文字后按快捷键 | 显示"无法读取以管理员身份运行的程序"，并提到连按两次 Ctrl+C | ✅ `precheck=elevated`，显示"无法读取以管理员身份运行的程序"（提示里提到的连按两次 Ctrl+C 尚未实现，见问题 R1-9） |
| 6.2 | LexTrail 自己 | 在主窗口中选中文字后按快捷键 | 显示"这个窗口暂时无法取词" | ✅ `precheck=unsupported`，显示"暂时无法取词" |
| 6.3 | 桌面 | 点击桌面空白处后按快捷键 | 显示提示，不报错 | ✅ 显示"暂时无法取词"（`uia=unsupported 5.6ms`）；改为"没有选中文字"更合适（见问题 R1-10） |
| 6.4 | 取词变慢 | 设 `LEXTRAIL_DEBUG_CAPTURE_DELAY_MS=800`，选中文字后按快捷键 | 约 100 ms 后显示"正在取词…"，原窗口保持焦点；取词完成后显示结果并获得焦点 | ⚠️ 3 次中 2 次正常；第 2 次 `SetForegroundWindow refused` → 后备做法 → `ForegroundChanged -> Hide`，悬浮窗只闪了一下就消失（见问题 R1-2） |
| 6.5 | 取词期间切换窗口 | 同 6.4，按快捷键后立即 Alt+Tab 到别的窗口 | 悬浮窗隐藏，之后不再出现 | ✅ 悬浮窗消失，之后不再出现 |
| 6.6 | 连续按快捷键 | 快速连按两次快捷键 | 只显示最后一次的结果，没有旧内容闪现 | ❌ 第一次显示 banana；第二次按下时前台是悬浮窗本身，`precheck=unsupported` 覆盖了结果（见问题 R1-5） |
| 6.7 | 冷启动 | 启动后第一次按快捷键 | 结果正常显示并获得焦点 | ✅ |
| 6.8 | 挂起后恢复 | 关闭悬浮窗等 10 秒以上再查 | 结果正常显示并获得焦点，没有旧内容闪现 | ✅ |
| 6.9 | 多显示器 / 高 DPI | 在副屏、缩放 150% 的屏幕上取词 | 悬浮窗出现在鼠标附近，内容清晰 | ⚠️ 两块屏幕缩放不同（都在启动前设好）时位置、清晰度、翻转都正确；运行中把外接显示器从 150% 改为 100% 后，两块屏幕上悬浮窗都约大 1.5 倍，改回或重启后恢复（见问题 R1-8） |
| 6.10 | 剪贴板兜底 | （剪贴板兜底完成后）在只能走兜底的程序中取词 | 取词完成后悬浮窗仍能获得焦点（模拟按键可能影响前台权限） | 未测（剪贴板兜底尚未实现） |

## 第一阶段修复后的复测（R2）

只需测下面这些项目（对应 R1-1、2、5、6、7、8、9、10，外加 3 项回归）。构建 debug 版（命令见第 0 节），在终端中运行；标 **[release]** 的项目再用 release 版、从资源管理器双击启动测一次。每项把结果和日志里对应的 `capture #N` 那一行填进表里。

| # | 对应 | 步骤 | 预期 | 结果 |
|---|---|---|---|---|
| R2-1 | R1-1 | **[release]** 用真实键盘：在 VS Code Markdown 预览中拖选一个词 → 快捷键 → 看到结果后按 Esc → 按 ↓。重复 3 次；再在编辑器和集成终端中各做 1 次 | 3 次都没有"文件(F)"被框住，按 ↓ 不弹出菜单；编辑器、终端里也没有 | ✅ 真实键盘下预览 3 次、编辑器、终端都没有框住"文件(F)"（debug 版、release 版） |
| R2-2 | R1-1 | Word、记事本、Edge 中各查一次词后按 Esc | 功能区不出现字母提示，记事本、Edge 不出现菜单栏激活 | ✅ Word 无 KeyTips，记事本、Edge 无菜单激活 |
| R2-3 | R1-2 | 设 `LEXTRAIL_DEBUG_SKIP_ACTIVATION=1` 启动 debug 版（模拟"无法获得焦点"）。在记事本中查词 | 结果照常显示；日志有 `activation skipped`；悬浮窗没有焦点 | ✅ 日志有 `activation skipped`，结果正常显示，记事本保持激活 |
| R2-4 | R1-2 | 接 R2-3：分别试 ① 点 × 按钮；② 点悬浮窗正文再按 Esc；③ 切换到别的窗口；④ 在记事本里继续打字 | ①②③都能关闭悬浮窗；④不关闭（在原窗口内操作不算切换），之后用 ①–③ 仍可关闭 | ✅ ×、点击后 Esc、切换窗口都能关闭；在原窗口打字不关闭 |
| R2-5 | R1-2 | 去掉上述环境变量，设 `LEXTRAIL_DEBUG_CAPTURE_DELAY_MS=800`，在记事本中查词 5 次（可在等待期间轻微移动鼠标） | 每次都先显示"正在取词…"，再显示结果；不会只闪一下就消失。日志 `bring to front:` 后为 `Direct` 或 `Attached`；如出现 `Refused`，结果仍显示且可用 × 关闭 | ✅ 6 次都先显示"正在取词…"再出结果，`bring to front` 全部为 `Direct`。未出现 `Refused`：`AttachThreadInput` 重试在真实环境中尚未触发过，"被拒后不带焦点显示"只经 R2-3 的模拟开关验证 |
| R2-6 | R1-5 | 查到一个词后，悬浮窗还在时再按一次快捷键 | 结果保留不变；日志有 `hotkey ignored: the popup is in front` | ✅ 连按 3 次都是 `hotkey ignored: the popup is in front`，结果保留 |
| R2-7 | R1-10 | 点击桌面空白处后按快捷键 | 显示"没有选中文字"；日志 `precheck=no_selection` | ✅ `precheck=no_selection`，显示"没有选中文字" |
| R2-8 | R1-9 | 以管理员身份运行记事本查词；在 LexTrail 主窗口中查词 | 提示文字不再提"连按两次 Ctrl+C"（分别为"Windows 不允许……"和"这个程序没有提供选中的文字。"） | ✅ 两处提示都不再提"连按两次 Ctrl+C" |
| R2-9 | R1-7 | Word 表格单元格中选词 | 句子末尾没有方块 | ✅ 句子末尾没有方块 |
| R2-10 | R1-6、R1-7 | Chrome 打开 `arxiv.org/pdf/1706.03762`，选 improving（句子含行末连字符的 "English-to-German"）；再选第 2 页其他段落的 2 个词 | 取到所选的词；句子显示 "English-to-German"，没有方块；日志没有 `[chromium_pdf]` | ✅ single-model、dependencies、sentence 都正确，无 `[chromium_pdf]`、无方块。dependencies 的句子里有 `transduc-tion`（原文行末断词），属预期：词典接入后由行末连字符任务合并 |
| R2-11 | R1-8 | **[release]** 运行中把外接显示器从 150% 改为 100%，在两块屏幕上各查一次词；再改回 150% 各查一次 | 每次悬浮窗大小都与该屏幕缩放相符（不再约大 1.5 倍） | ✅ release 版运行中 150%→100%→150%，两块屏幕上悬浮窗大小都正确 |
| R2-12 | 回归 | Edge 网页查词（1.1）、Word 查词（3.1）、刚启动后第一次查词（6.7） | 与第一轮结果相同 | ✅ Edge、Word 正常；release 版启动后第一次查词正常 |

## 第二阶段（剪贴板兜底）后的复测（R3）

只需测下面这些项目。构建 debug 版运行；标 **[release]** 的再用 release 版、从资源管理器双击启动测一次。测之前先复制一段自己认得出的文字（例如"R3 原剪贴板"）到剪贴板，每项测完检查剪贴板内容仍是它（"剪贴板保持"一栏）。日志中剪贴板这一步显示为 `clipboard=...`，用 Ctrl+Insert 时带 `[ctrl_insert]`。

| # | 对应 | 步骤 | 预期 | 结果 |
|---|---|---|---|---|
| R3-1 | 2.1、R1-4 | **[release]** Edge 打开本地 PDF，选中一个单词 → 快捷键 | 取到单词（没有句子行）；日志 `uia=unsupported ... [chromium_pdf]; clipboard=captured`；悬浮窗获得焦点；剪贴板保持 || ✅ debug 版、release 版（资源管理器双击启动）都取到单词，悬浮窗获得焦点，剪贴板保持 |
| R3-2 | 2.2 | Edge PDF 中选中跨行短语 | 取到短语，换行合并为空格 || ✅ |
| R3-3 | R1-4 | Chrome 刚打开 PDF 时立即选词查词（第一轮出现过 `no_selection`） | 取到单词：UIA 说没选中时由剪贴板确认（`uia=no_selection ...; clipboard=captured`）；剪贴板保持 || ✅ Chrome 刚打开 PDF 时出现过 `uia=no_selection 83.8ms x3; clipboard=captured 59.5ms`，剪贴板确认生效 |
| R3-4 | 4.1、R1-3 | VS Code 编辑器中选中一个单词 | 取到单词（`uia=no_selection; clipboard=captured`）；剪贴板保持 || ✅ |
| R3-5 | 4.1 | VS Code 编辑器中什么都不选（光标停在某行） | 显示"没有选中文字"（`[vscode_empty_selection]`），不显示整行；剪贴板保持 || ✅ 3 次都是 `clipboard=no_selection [vscode_empty_selection]` |
| R3-6 | 4.3、R1-3 | VS Code 集成终端中选中一个单词 | 取到单词（`[ctrl_insert]`）；剪贴板保持 || ✅ |
| R3-7 | 4.3 | VS Code 集成终端中运行 `ping -t 127.0.0.1`，什么都不选，按快捷键 | 显示"没有选中文字"；ping 继续运行，没有被中断 || ✅ `clipboard=no_selection 550.8ms [clipboard_unchanged]`，显示"没有选中文字"，ping 未被中断 |
| R3-8 | 5.3 | 旧版控制台（conhost）中选中一个单词；再什么都不选按一次 | 取到单词（`[ctrl_insert]`）；不选时显示"没有选中文字"，命令行里没有多余字符 || ✅ |
| R3-9 | 5.1 | Windows Terminal 中选词 | 仍由 UIA 取到（日志里没有 `clipboard=`） || ❌ UIA 报 `no_selection`（4/4），查完选区高亮消失。设 `LEXTRAIL_DEBUG_NO_ALT_MASK=1` 后 3/3 取到、选区保留：屏蔽键 `0xE8` 让 Windows Terminal 清掉了选区（R1-1 的修复引入，R2 没测 WT；见问题 R3-P1） |
| R3-10 | B2 | 记事本、Word 中什么都不选按快捷键 | 显示"没有选中文字"，日志里没有 `clipboard=`（可靠程序不做剪贴板确认），响应很快 || ✅ |
| R3-11 | D043 | 做完 R3-1 后按 Win+V 打开剪贴板历史 | 有一条目标程序复制的单词（已知限制，无法避免）；**没有**第二条"R3 原剪贴板"（LexTrail 写回的不进历史） || ✅ 测前清空了 Win+V 历史。写回的原内容没有进历史；这次 Edge 复制的单词也没有进历史（可能是时序原因，D043 仍保留"可能进入"的说法） |
| R3-12 | D043 | 复制一张图片（如截图）到剪贴板，再在 Edge PDF 中查词，然后粘贴到画图 | 粘贴出的仍是原来的图片 || ✅ |
| R3-13 | D043 | 复制一个文件（资源管理器中 Ctrl+C），再在 Edge PDF 中查词，然后在别的文件夹粘贴 | 粘贴出的是原来的文件 || ✅ |
| R3-14 | 回归 | Edge 网页（1.1）、Word（3.1）各查一次 | 与之前相同，日志里没有 `clipboard=` || ✅ |

## R3 问题修复后的复测（R4）

对应 R3-P1 至 R3-P4。构建 debug 版运行；标 **[release]** 的再用 release 版、从资源管理器双击启动测一次。**R4-1 至 R4-4、R4-8 必须用真实键盘按快捷键。** debug 版日志的变化：

- 每次取词一行多了 `start +N ms`（从按下快捷键到开始取词），剪贴板这一步多了 `keys=N ms`（等松开快捷键的时间），例如 `capture #3: captured | start +7.6ms; uia=unsupported 3.1ms ...; clipboard=captured 218.4ms keys=208.5ms; total 221.6ms`。
- 每次补发屏蔽键一行：`hotkey: alt mask: repeat LeftCtrl (...)`，只有 Alt 时为 `alt mask: 0xE8`。如果出现 `was released before its mask repeat; released it`，说明遇到了松开与补发的竞争并已处理，请记下当时的操作。

| # | 对应 | 步骤 | 预期 | 结果 |
|---|---|---|---|---|
| R4-1 | R3-P1、5.1 | **[release]** Windows Terminal 中用鼠标选中一个单词 → 快捷键，重复 3 次；再选跨行文字 1 次 | 取到单词（日志 `uia=captured`，没有 `clipboard=`）；查词后 Windows Terminal 里的选区高亮**还在** | |
| R4-2 | R2-1 | **[release]** VS Code Markdown 预览中拖选一个词 → 快捷键 → 看到结果后按 Esc → 按 ↓，重复 3 次；再在编辑器和集成终端中各做 1 次 | 没有"文件(F)"被框住，按 ↓ 不弹出菜单 | |
| R4-3 | R2-2 | Word、记事本、Edge 中各查一次词后按 Esc | Word 不出现 KeyTips，记事本、Edge 不出现菜单栏激活 | |
| R4-4 | R3-P3、P1 | 退出 LexTrail 后重新启动，**启动后的第一次查词在 VS Code 编辑器里做**（选中一个单词）→ 看到结果后按 Esc → 按 ↓ | 取到单词；菜单栏没有被激活。日志 `start +` 在 20 ms 以内；`keys=` 与之后几次相近（约 100–250 ms，取决于松手快慢），不再是 10 ms 左右 | |
| R4-5 | R3-P3 | 再退出、重新启动，第一次查词在 Edge PDF 里做 | 同 R4-4：取到单词；`keys=` 与之后几次相近 | ✅（AI 模拟按键，2026-10-20，debug 版）`popup-e2e.ps1 -Pdf -HoldMs 200 -Lookups 3`，启动后第 1 次：`start +19.3ms; uia=unsupported 53.1ms x2 [chromium_pdf]; clipboard=captured 170.6ms keys=158.0ms`；第 2、3 次 `keys=196.7ms`、`198.2ms`。三次都取到 launch，悬浮窗获得焦点，截图核对 |
| R4-6 | R3-P2、P4 | Edge PDF 中选中单词，按快捷键后**按住 Ctrl+Alt 约 2 秒**再松开；再做一次按住超过 3 秒 | 约 2 秒：按住约 1 秒后显示"松开快捷键后取词…"，松手后显示单词；超过 3 秒：显示"请松开快捷键后再试"（不再是"取词超时"），剪贴板保持 | |
| R4-7 | R3-P4 | **[release]** Edge PDF、VS Code 编辑器中正常快速按一下快捷键，各 3 次 | 直接显示结果，不再先闪"正在取词…" | |
| R4-8 | D014 补充 | 在记事本里**快速连按快捷键 20 次**（每次看到悬浮窗就按 Esc），然后在记事本里打几个字母 | 打出来的是字母（Ctrl 没有卡住，没有触发 Ctrl+字母的快捷键）；日志中如有 `released first`，记下次数 | |
| R4-9 | R3-7 | VS Code 集成终端中运行 `ping -t 127.0.0.1`，什么都不选，按快捷键 | 显示"没有选中文字"；ping 继续运行，没有被中断 | ✅（AI 模拟按键，2026-10-20，debug 版，单独的 VS Code 测试配置）悬浮窗显示"没有选中文字"（截图 + UIA 读回）；`uia=no_selection 54.9ms x2; clipboard=no_selection 511.2ms keys=101.2ms [clipboard_unchanged]`；前后截图对比 ping 持续输出，没有统计行或 `^C`，ping 进程仍在；剪贴板保持 |
| R4-10 | 回归 | Edge 网页（1.1）、Word（3.1）、conhost 选词（R3-8）各查一次 | 与之前相同 | ✅（AI 模拟按键，2026-10-20，debug 版，真实鼠标双击选词）Edge 网页：launch + 句子，`uia=captured 56.3ms x2`；Word：arrived + 句子，`uia=captured 21.2ms`，查词后选区保留、没有 KeyTips；conhost：address，`clipboard=captured 155.7ms keys=138.0ms [ctrl_insert]`，命令行没有多余字符，剪贴板保持。三项都截图核对，日志里没有 `released first` |
| R4-11 | R4-P1 | 悬浮窗挂起后查词：每次查完按 Esc，**等 6 秒以上**（悬浮窗挂起，debug 日志 `SuspendFinished { suspended: true }`）再查下一次；Word、记事本各 5 次 | 每次都显示结果并获得焦点；之后快捷键、托盘仍有响应（R4-P1 中第 3 次查词后程序卡死） | |

AI 用模拟按键做的 R4-P1 回归检查（2026-10-20，debug 版，修复后）：`popup-e2e.ps1 -Lookups 3 -PauseMs 7000 -ResumeDelayMs 300`（挂起后恢复，并用 `LEXTRAIL_DEBUG_RESUME_DELAY_MS` 让 Resume 变慢，使取词结果先于 `WindowReady` 到达）——**修复前第 2 次查词卡死**（`responds=False`，日志停在 `WindowReady -> [SetContent, ShowInactive, Activate]`，与 R4-P1 相同），修复后 3 次全部通过；另外 `-PauseMs 7000`（网页）、`-Pdf -HoldMs 200 -PauseMs 7000`、默认连续 3 次，以及 Word 中挂起后连查 3 次，全部通过，悬浮窗内容用 UIA 读回、截图核对。

AI 已用模拟按键做过的自动检查（2026-10-17，debug 版）：`popup-e2e.ps1` 网页、`-Pdf`、`-Pdf -HoldMs 200`（3 次，均未出现"正在取词…"）、`-Pdf -HoldMs 2000`（按住期间显示"松开快捷键后取词…"，截图核对）、`-Pdf -HoldMs 3600`（显示"请松开快捷键后再试"，截图核对）全部通过；`stuck-keys.ps1` 120 次按键没有修饰键卡住，其中 1 次触发了"先松开后补发"的竞争并已补发松开。模拟按键不能代替 R4-1 至 R4-4、R4-8 的真实键盘测试。

## 记录

| 日期 | 测试人 | Windows 版本 | 备注 |
|---|---|---|---|
| 2026-10-16 | 项目负责人 | Windows 11 | 第三轮（R3）：剪贴板兜底后的复测，debug 版全项 + release 版 R3-1。13 项通过，R3-9（Windows Terminal）失败；另发现问题 R3-P1 至 R3-P4（见下） |
| 2026-10-16 | 项目负责人 | Windows 11 | 第二轮（R2）：第一阶段修复后的复测，12 项全部通过，第一阶段验收 |
| 2026-10-16 | 项目负责人 | Windows 11 | 第一轮（R1）：debug 版全项 + release 版（资源管理器双击启动）复查 Edge、Word、VS Code 预览。上表"结果"列为本轮结果 |

### 第一轮发现的问题

| 编号 | 问题 | 处理 |
|---|---|---|
| R1-1 | VS Code 预览按 Esc 后焦点停在菜单栏（"文件(F)"被框住） | 第一阶段 A3：确认原因，补发屏蔽键 `vkE8` |
| R1-2 | 激活失败时，后备做法引起的前台变化被当成用户切换窗口，悬浮窗被隐藏 | 第一阶段 A2：改用 `AttachThreadInput`；激活失败时不带焦点显示结果 |
| R1-3 | VS Code 编辑器、集成终端已选中却返回 `no_selection` | 第二阶段 B3：查原因（辅助功能支持设置），由剪贴板兜底覆盖 |
| R1-4 | UIA "没选中"其实是读错（VS Code、刚打开的 Chrome PDF） | 第二阶段 B2：对 UIA 不可靠的程序用剪贴板确认，记入 DECISIONS |
| R1-5 | 悬浮窗在前台时再按快捷键，结果被"暂时无法取词"覆盖 | 第一阶段 A1：忽略这次按键，保留当前结果 |
| R1-6 | Chrome 的 PDF 没有被识别为 `[chromium_pdf]` | 第一阶段 A7：用 `uia_dump` 检查是否错位后再决定 |
| R1-7 | 上下文中的控制字符显示为方块（Chrome PDF 的 `\u{2}`、Word 表格的 `\u{7}`） | 第一阶段 A5：过滤控制字符；`\u{2}` 还原为 `-`，去掉后词典里查得到则合并（M1） |
| R1-8 | 运行中改显示器缩放后悬浮窗尺寸不更新 | 第一阶段 A6：每次显示时按目标显示器重新设置尺寸 |
| R1-9 | 提示文字提到尚未实现的"连按两次 Ctrl+C" | 第一阶段 A4：先去掉，功能完成后再加回并检查 |
| R1-10 | Windows Terminal 跨行时句子带提示符；桌面显示"暂时无法取词" | 桌面改为"没有选中文字"（A1）；提示符以后处理 |
| R1-11 | 句子开头的 "..."；源代码文本框中上下文是很长的一整行 | 已在 ROADMAP（低优先级） |

### 第三轮发现的问题

| 编号 | 问题 | 处理 |
|---|---|---|
| R3-P1（高） | Windows Terminal 选词后查词，UIA 报 `no_selection`（4/4），选区高亮消失；关掉屏蔽键后 3/3 正常。屏蔽键 `0xE8` 是普通按键，Windows Terminal 收到非修饰键就清除选区（R3-9） | 已修复：改为补发仍按着的修饰键（Ctrl / Shift）的按下，低级键盘钩子检测"先松开"的竞争并补发松开（D014 补充）。待 R4-1 至 R4-4、R4-8 复测 |
| R3-P2（中） | 走剪贴板的程序（Edge PDF、VS Code）中按住 Ctrl+Alt 超过约 0.6 秒：`clipboard=timed_out 604ms [modifiers_held]`，提示"取词超时 / 程序响应较慢"，原因不对，松手后也不补取（走 UIA 的程序按住也能取词） | 已修复：等待改为最多 3 秒，超过时结果为 `keys_held`，提示"请松开快捷键后再试"（D043 补充）。待 R4-6 复测 |
| R3-P3（待查） | 两轮 debug 测试中，启动后第一次取词的剪贴板步骤只用约 10 ms（如 `uia=unsupported 12.4ms; clipboard=captured 12.5ms; total 25.0ms`），之后每次 150–210 ms（在等松开修饰键，已用实验确认）。怀疑第一次没等修饰键 | 原因：第一次查词时创建悬浮窗在快捷键线程上同步阻塞，取词线程在那之后才启动，用户已经松手，所以几乎不用等（复制本身是正确的 Ctrl+C，否则 Edge PDF 不会复制成功）。已改为先启动取词线程（D042 补充）；日志加了 `start +N ms` 和 `keys=N ms`。待 R4-4、R4-5 复测 |
| R3-P4（观察） | 走剪贴板时每次都要等松开修饰键（约 150–200 ms），所以都会先闪一下"正在取词…"再出结果（release 版确认） | 已修复：约 100 ms 的计时在松开快捷键时重新开始；按住超过约 1 秒显示"松开快捷键后取词…"（D042 补充）。待 R4-6、R4-7 复测 |

### 第四轮发现的问题

| 编号 | 问题 | 处理 |
|---|---|---|
| R4-P1（严重） | debug 版 `b4ba367`：悬浮窗挂起后在 Word 中查词，悬浮窗没有出现，之后快捷键、托盘都无响应（其他程序正常）。日志停在 `request: WindowReady -> [SetContent(...), ShowInactive, Activate]`。原因：R3-P3 改为先启动取词后，恢复路径上第一次出现"取词结果先于 `WindowReady`"的顺序（UIA 7.9 ms 比 Resume 快）。`webview::resume_then` 在 `window.with_webview` 的回调里同步调用 `then()`，于是 `WindowReady` 返回的动作都在回调里执行；Tauri 在主线程上调用 `with_webview` 时，回调执行期间一直持有该 webview 的 `window_id` 锁，而第一个动作 `SetContent`（`emit_to` → `eval_script`）要再拿同一把锁，主线程自己等自己。用临时日志确认卡在 `SetContent` 内部；锁的位置见 `tauri-runtime-wry` 2.12.2 `with_webview` / `eval_script` | 已修复：`with_webview` 回调里不再调用任何应用代码，`resume_then` 的 `then` 和 `suspend` 的完成通知都改到新线程上、回调返回之后执行（`popup/webview.rs` 模块注释写明了这条规则）。新增 debug 开关 `LEXTRAIL_DEBUG_RESUME_DELAY_MS` 和 `popup-e2e.ps1 -PauseMs / -ResumeDelayMs`（每次查词后检查 UI 线程是否响应），修复前稳定复现、修复后通过。待 R4-11 及重做的 R4 复测 |

记录，暂不处理：
- Word 中什么都不选时偶发一次 `uia=timed_out 193.7ms`，随后剪贴板兜底给出 `no_selection [clipboard_unchanged]`，共 598 ms；之后重测 3 次都是 3.6–5.5 ms，未复现。
- Word 中出现过一次句子开头少字：`text: "jumps", sentence: "he quick brown fox jumps over the lazy dog."`。原文档已无法确认；在新文档第一行输入同一句重测，`jumps`、`The` 的句子都完整，未复现。
- Word 句子切分正确处理了 `Mr.`、`3.14`、`p.m.`、`e.g.`。
