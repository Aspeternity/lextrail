# M1 悬浮窗显示词典结果

> 状态：**已确认并实现**（2026-10-20，分支 `feat/popup-dict`）。项目负责人在草稿基础上的修改见 D046 和 D044 补充（只合并确认是行末断词的连字符、Edge PDF 用猜测规则）。相关决定：D015、D018、D019、D044、D045、D046。

## 一、查到整词时（单词、短语）

```
launch  /lɔ:ntʃ/  四级 六级 考研 托福 雅思 牛津 ★★★★
n. 下水；汽艇；发射
vt. 使下水；发射；发动
vi. 起飞；下水；投入；开始
▸ 专业释义（3）
──────────────────────────────
原句（最多 2 行）
AI 语境解析：未配置（M2 接入）            来自 msedge.exe
```

- **标题行**：词头、音标（ECDICT 只有一个音标，不标英 / 美）；考试标签显示为中文（中考、高考、四级、六级、考研、托福、雅思、GRE，按这个顺序）；牛津 3000 核心词显示"牛津"；柯林斯星级显示为 ★。
- **义项**：按原文顺序，相邻的同一词性义项合成一行，用"；"连起来。数据里每个义项仍是独立的一条，M2 的 AI 排序（D019）不受影响。
- **专业释义**：带领域标记的释义（[计]、[医] 等）默认折叠为"▸ 专业释义（N）"，点击展开 / 收起。**词条没有普通释义、只有专业释义时默认展开**（D046），正文不会为空。
- **滚动**：窗口大小不变（380×260 逻辑像素）；内容超出时正文区域滚动，词条标题滚动时固定在顶部，底部的原句和来源固定。滚动条的轨道从关闭按钮下方开始。
- **多个词条**（变形词与原形、大小写两个词条）：每个词条先显示前 3 个词性行，多出的点"▸ 更多（N）"展开。变形词条的标题写成"better（good 的比较级）"。只给原形时（变形词条只是"xx 的过去式"说明），原形词条上方加一行灰字"launched 是 launch 的过去式、过去分词"。
- **大小写两个词条**（March / march、The / the）：和选中文字大小写完全一致的在前；选中的词在句首时（有原句时才能判断，`lextrail_core::text::starts_sentence`）小写的在前（D046）。
- **原句**：最多 2 行，超出省略；只在 UIA 取词时有。
- **没有复制按钮**（不在 M1，D046）。

## 二、带连字符的复合词拆开时

```
self-attention
词典没有整词，以下是各部分的释义
self  /self/  n. 自己；自我；本性；…
attention  /ə'tenʃən/  n. 注意；注意力
未收录：xxx
```

每个部分只显示第一行（没有普通释义时用第一行专业释义），最多 2 行，超出省略；部分本身是变形时带关系说明（"models（model 的复数）"）。

## 三、其他情况

- 词典查不到：原文 + "词典中没有收录。"
- 句子：原文（最多 4 行）+ "句子翻译需要 AI（M2 接入）。"
- 没有英文字母：标题"目前只支持英文" + 原文（D045）。
- 词典文件缺失或格式不对：原文 + "词典文件缺失，暂时无法查词。"，日志写一次原因；取词照常，下次查词时再试着打开。

## 四、处理顺序（`apps/desktop/src-tauri/src/popup/content.rs`）

都在取词线程里完成，结果放进发给页面的 `Content::Selection.lookup`（`lextrail_dict::view::LookupView`）：

1. 取词；从原始上下文切出句子（`sentence_around`）。
2. 合并行末断词，选中的文字和句子都做（D044 补充）：
   - PDFium 的标记 `\u{2}`：`resolve_line_end_hyphens`，合起来是词才合并，否则还原为 `-`。
   - 经剪贴板从 Chromium PDF 窗口取到的文字（`CaptureTrace::from_chromium_pdf`，即 Edge PDF）：`join_unmarked_line_breaks`，左半段单独不是词、合起来是词时去掉换行。
   - "是不是词"用 `Dictionary::is_word`：整词查询，或完整版 ECDICT 词头表 `extra_words`（直接或经后缀规则）；拆开查到的复合词不算。
3. 规范化（`normalize_text`）。
4. 分类（D045）并查词（`Dictionary::view`：`lookup_with_parts` + 显示整理）。

## 五、词典文件与常驻

- 文件 `data/build/lextrail-dict-core.db`（`cargo run -p lextrail-dict-builder --release` 生成），作为 Tauri 资源复制到可执行文件旁（`tauri.conf.json` 的 `bundle.resources`；debug 构建和 `tauri build --no-bundle` 都会复制）。**编译桌面程序前必须先生成词典**，否则 `tauri-build` 报错；CI 的 Windows job 在 clippy 之前生成（有缓存）。
- 格式版本 2：新增表 `extra_words(word)`，完整版 ECDICT 中纯 ASCII 字母、至少 2 个字母、不是精简版词条（不分大小写）的单词，小写存放，165,864 个；文件 33.4 → 35.7 MiB。
- 第一次取到文字时打开（只有托盘时不打开），之后一直打开（D046）。`PRAGMA cache_size = -1024`（1 MiB），`PRAGMA mmap_size = 0`。
- 查词耗时（debug 版日志 `lookup N ms`）：第一次（含打开词典）约 1.4 ms，之后 0.2–1.5 ms。实现中发现原形查询的连接用了区分大小写的比较，用不上索引，每次扫描全部词条（约 30 ms），已改为 `COLLATE NOCASE`，并有测试检查查询计划。

## 六、行末连字符：两种取词方式实际取到的文字（2026-10-20 实测）

在 Chrome 和 Edge（InPrivate，单独的配置目录）中打开 `1706.03762v7.pdf`，用真实鼠标拖动选中跨行的词，`capture_probe` 打印（Rust 转义）：

| 位置 | Chrome PDF（UIA） | Edge PDF（剪贴板） |
|---|---|---|
| transduc-tion | 选中 `" transduc\u{2}tion "`；上下文 `…and transduc\u{2}tion models in…` | `" transduc\r\ntion m"` |
| convolu-tional | `" convolu\u{2}tional"`；上下文 `…and convolu\u{2}tional layers…` | `" convolu\r\ntional l"` |
| English-to-German | `" English\u{2}to-Germa"`；上下文 `…WMT 2014 English\u{2}to-German translation…` | `"English\r\nto-Germa"` |
| sequence-aligned（真复合词断在行末） | `"sequence\u{2}aligned "` | `"sequence\r\naligned"` |
| 普通换行 | `\r\n` | `"positions\r\nof "` |

- Chrome：行末连字符一律是 `\u{2}`，后面没有换行；行内连字符（End-to-end、simple-language、self-attention）是普通 `-`。
- Edge：行末连字符被丢掉，纯文本和 HTML 格式里都只有换行，和普通换行无法区分。
- 猜测规则在全文上的效果（`pdftotext -raw`，精简版 + 词头表，近似后缀规则）：16 处行末连字符中真断词 6 个，合并 transduction、transformation、convolutional、Convolutional，漏掉 multi-plicative、sur-prisingly、im-age（左半段 multi、sur、im 本身是词条）；465 处普通换行误合并 0。

## 七、验证

- 自动测试：`lextrail-core`（`join_unmarked_line_breaks`、`starts_sentence`）、`lextrail-dict`（`is_word`、大小写顺序、`view`、查询计划）、`dict-builder`（`extra_words`）、桌面端 `content.rs`（选中文字与句子的合并、Edge PDF 剪贴板、大小写顺序、没有词典时）。
- 截图核对（AI 模拟按键，debug 版，2026-10-20）：Chrome PDF transduc-tion（合并，句子也合并，点开专业释义后按 Esc 关闭、焦点回到 Chrome）；Edge PDF transduc-tion（合并）；网页中的 better、left（更多）、launched、self-attention（拆开）、qwzxv（没有收录）、纯中文、句子、run（滚动）、March。
- 手动测试清单：`m1-capture-manual-tests.md` 第五轮（R5）。
