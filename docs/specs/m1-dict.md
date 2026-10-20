# M1 词典：dict-builder 与词典数据格式

> 相关决定：D015、D019、D021、D038、D044。第 3、4 节和音标规范化由项目负责人确定（2026-10-20）。

## 1. 数据来源（D038）

| 项 | 值 |
|---|---|
| 仓库 | `skywind3000/ECDICT`（MIT） |
| 文件 | `ecdict.csv`（"基础版"，77 万词条） |
| 固定版本 | 提交 `82c9872576b23118d7c42e920c11beb77f510ae2`（2025-01-02，这个文件最后一次修改） |
| 下载地址 | `https://raw.githubusercontent.com/skywind3000/ECDICT/82c9872576b23118d7c42e920c11beb77f510ae2/ecdict.csv` |
| 大小 | 65,933,428 字节 |
| SHA-256 | `1a6947e04785db63613a92e14903cdae7954f7e84860b10e68e5c7cbb3f9c3cf` |

不用 GitHub Release 中的数据包：最新的 Release（1.0.28）是 2017 年的，之后的修正（如 2025-01 清理 `exchange` 字段）只在仓库里。

下载到 `data/raw/`，生成结果放 `data/build/`（两者都在 `.gitignore` 中）。下载后先校验大小和 SHA-256，不符合就删除并报错，不做任何生成。

## 2. ECDICT 字段情况（按上面的固定版本统计）

| 字段 | 非空词条数 | 用途 |
|---|---|---|
| `word` | 770,611（其中含空格的短语 366,599） | 词头 |
| `phonetic` | 218,065 | 音标（只有一个，不分英美；约 10 万条用的是西里尔字母 `ә` 而不是 IPA 的 `ə`） |
| `translation` | 768,739 | 中文释义，按词性分行（`\n`），如 `n. 实数, 现实\nadv. 真正地`；带领域标记的行如 `[医] …`、`[网络] …` |
| `definition` | 160,884 | 英文释义（第一版不用） |
| `tag` | 14,942 | 考试标签：`zk gk cet4 cet6 ky toefl ielts gre` |
| `collins` / `oxford` | 13,633 / 3,461 | 柯林斯星级、是否牛津 3000 核心词 |
| `bnc` / `frq` | 45,443 / 42,231 | BNC / 当代语料库词频排名 |
| `exchange` | 96,290 | 词形变化：`p:` 过去式、`d:` 过去分词、`i:` 现在分词、`3:` 第三人称单数、`s:` 复数、`r:` 比较级、`t:` 最高级、`0:` 原形（lemma）、`1:` 原形的变化类型 |
| `pos`、`detail`、`audio` | 0 / 1 / 0 | 不用 |

## 3. 精简版收录哪些词条

只有约 5.9 万词条带有词频、考试标签、柯林斯或牛津标记，其余大多是专业术语和短语。几个候选方案（"原始大小"是所需字段的 UTF-8 字节数，SQLite 文件加上索引后大约再大 30–60%）：

| 方案 | 收录规则 | 词条数 | 原始大小 |
|---|---|---|---|
| A | 有词频 / 考试标签 / 柯林斯 / 牛津标记 | 59,137 | 4.9 MB |
| B | A + 所有带音标的单词（不含空格） | 211,861 | 14.4 MB |
| B+ | B + 释义不以领域标记 `[..]` 开头的短语 | 约 27 万 | 约 17 MB |
| C | 全部（有中文释义的） | 768,739 | 40.3 MB |

**结论：方案 B+**。规则：有中文释义，并且满足以下任一条件：
1. 有词频（`bnc` 或 `frq` 不为 0）、考试标签、柯林斯星级或牛津标记；
2. 不含空格（单词）且有音标；
3. 含空格（短语），且释义第一行不以领域标记 `[` 开头。

参考：`transduction`、`computation` 在 A 中就有；`give up`、`in terms of`、`take into account`、`run out of` 不在 A 也不在 B，只在 B+ / C 中；`lemmatization`、`tokenizer` 在 ECDICT 中都没有（交给 AI）。

## 4. 义项拆分

`ARCHITECTURE.md` 5.2 写的是"逗号分隔的近义说法属于同一义项"。但 ECDICT 中有两种写法：

- 新写法：义项之间用分号 `；`，同一义项内的近义说法用逗号，如 `v. 为王，为君；当政；统治`。
- 老写法（大多数常用词）：全部用逗号，如 `a. 真的, 真实的, 实际的, 实在的, 不动(产)的, 实数的`，其中"实数的"显然是另一个义项。

**规则**：
1. 按行拆（`translation` 中的 `
`）；每行开头的领域标记 `[医]` 记为 `domain`，词性 `n.`、`vt.` 等记为 `pos`（只认已知词性缩写，且后面必须是空格、连接符或行尾，避免把 `a.m.` 这类正文误认作词性）。多个词性统一写成 `vt. & vi.`（原文有 `vt.,vi.`、`vt.& vi.`、`n./v.` 等写法）。
2. 行内有分号（`；` 或 `;`）时**只按分号拆**，逗号视为同一义项内的近义说法；
3. 整行没有分号时按逗号（`，` 或 `,`）拆。
4. 括号（`()`、`（）`、`[]`、`【】`）内的分隔符不拆；两边都是数字的英文逗号（如 `3,000`）不拆；拆出的空段丢弃。

## 4.1 音标

只做一处规范化：西里尔字母 `ә`（U+04D9）替换为 IPA 的 `ə`（U+0259），其余保持原样。ECDICT 只有一个音标，不分英美。

## 5. 输出格式

SQLite 文件 `data/build/lextrail-dict-core.db`，只读，查询时按需从磁盘读（不整体载入内存）。表：

- `meta(key, value)`：`format_version`（数据格式版本，`lextrail-dict` 遇到不认识的版本拒绝打开）、`source`、`source_commit`、`source_sha256`、`selection`（收录规则名）。
- `entries(id, word, phonetic, collins, oxford, bnc, frq, tags)`：`word` 保留原始大小写；按 `word COLLATE NOCASE` 建索引。
- `senses(entry_id, ord, pos, domain, text)`：拆分后的义项，按原文顺序；`pos` 为词性（如 `n.`、`vt. & vi.`，可为空），`domain` 为领域标记（如 `医`、`网络`，可为空）。
- `forms(form, lemma, kind)`：由 `exchange` 生成的"变形 → 原形"表，用于词形还原；`kind` 为 ECDICT 代码（`p d i 3 s r t`）；来自所有行（包括没收录的变形词条自己的 `0:` 原形），只保留原形已收录的；按 `form COLLATE NOCASE` 建索引。
- 所有表 `STRICT`；数值字段为 0 / 空时存 `NULL`；音标见 4.1；考试标签去掉多余空格。

同一份输入、同一版 dict-builder 生成的文件内容应完全相同（固定插入顺序、不写生成时间），便于 CI 缓存和比对。

## 6. 代码位置

- 义项拆分、`exchange` 解析等与数据格式相关的纯函数放在 `crates/lextrail-dict`（D038：与 dict-builder 共用），dict-builder 依赖它。
- `tools/dict-builder`（crate `lextrail-dict-builder`）：下载、校验、读取 CSV、按收录规则筛选、写 SQLite。命令行：`cargo run -p lextrail-dict-builder --release`（默认下载 + 生成），`--offline` 只用已下载的文件。

## 7. 生成结果（2026-10-20，固定版本 `82c9872`）

| 项 | 值 |
|---|---|
| 读入行数 | 770,611 |
| 收录词条 | 277,936（另有 1 条释义拆不出义项，丢弃） |
| 义项 | 530,878 |
| 词形（`forms`） | 63,757 |
| 文件大小 | 33.4 MiB |
| 生成用时 | 约 1.5 秒（release，不含下载） |

两次生成的文件 SHA-256 相同（输出是确定的）。抽查：`real` 拆为 6 个形容词义项 + 2 个名词义项 + 1 个副词义项；`reign` 保留"为王，为君"为同一义项；`launching → launch`、`better → good`。

已知情况：
- 约 1,700 个没有词性的义项以"字母 + 点"开头，多为词缀词条的 `comb.form` 等写法，原样保留。
- 词缀词条（`-an`、`-ant` 等）的义项较长，带 `[构成形容词]` 之类的标记（记为 `domain`）。
- 33.4 MiB 比统计时估计的略大（义项拆开后每行都有主键和词性）；安装包压缩后的大小到 M6 打包时再看。

## 8. 查询与词形还原（`lextrail-dict`）

`Dictionary::open` 只读打开词典文件，检查 `meta.format_version`，不认识的版本（或不是词典文件）拒绝打开。`Dictionary::lookup(text)`：

1. **规范化**：去掉首尾空白和标点（引号、括号、句号等），弯引号 `’` 改为 `'`，去掉所有格 `'s`，中间的连续空白合并为一个空格。
2. **直接查**（不区分大小写）：大小写完全相同的词条优先，其次小写词条，再其次按来源顺序第一条。**首字母大写的词**（`March`、`The`）同时有小写和大写词条时两个都给，小写在前（句首大写很常见）。
3. **`exchange` 词形**（`forms` 表）：查到原形时一并给出原形词条；有多个原形（`found` → find / found）时取词频更高的。
4. **后缀规则兜底**（只在 3 查不到、且直接查不到或查到的只是"xx 的过去式"之类说明时）：`-ies/-ied/-ier/-iest → -y`、`-ying → -ie`、`-es`、`-s`、`-ed`、`-ing`、`-er`、`-est`（含双写辅音 `stopped → stop`、补 `e` 的 `making → make`）。候选原形必须在词典里，按顺序取第一个；`-er/-est` 只在原形有形容词义项时才算比较级 / 最高级（`optimizer` 不还原为 `optimize`）。
5. **短语**：直接查不到时，把每个词换成 `exchange` 原形再查（`gave up` → `give up`）。

结果：
- `entries`：查到的词条自己的词条（如果它不只是"xx 的过去式"之类说明），然后是原形的词条，每个都带角色：
  - `Query`：查的词本身；
  - `FormOf`：查的词本身、同时是另一个词的变形——**显示时必须标出和原形的关系**，如"left（leave 的过去式、过去分词）""better（good 的比较级）"（`Inflection::describe_zh()`）。否则像 `found`（自身释义"建立"，但多数时候是 find 的过去式）这种情况，变形在前会误导；
  - `Lemma`：原形的词条。

  例如 `launched` 只给 `launch`；`better` 给 `better`（good 的比较级）和 `good`。两个词条按语境排序留到 M2（D019）。
- `inflection`：原形、词形种类（过去式、复数等）和来源（`exchange` / 后缀规则 / 短语）。
- `lemma()`：写入查询历史 `lookups.lemma` 的原形。

### 8.1 带连字符的复合词（D044 补充）

`Dictionary::lookup_with_parts(text)` 返回 `Found::Whole`（整词查到，同 `lookup`）或 `Found::Parts`（拆开查到）：
- **只在整体查不到时拆**（按 `-` 拆），结果里标明"词典没有整词，以下是各部分的释义"；
- 跳过虚词（a、an、the、and、or、but、nor、of、to、in、on、at、by、for、from、with、as、into、onto、via、vs、per），不显示它们的词条；重复的部分只查一次；每个部分照常做词形还原；查不到的部分列在 `not_found`；
- 所有部分都查不到时返回 `None`。

**判断"是不是一个词"只能用 `lookup`（整词）**，不能用 `lookup_with_parts`。`lextrail_core::text::resolve_line_end_hyphens` 合并行末连字符时只能根据整词查到的结果判断：`compu-tation` 合并为 computation，`English-to-German`、`self-attention` 保持原样（测试 `line_end_hyphens_join_only_whole_words`）。
- 已知限制：词典里没有的词，行末断词不会被合并（覆盖率检查中 `convolu-` / `tional`，因为 convolutional 不在精简版中）。

## 9. 覆盖率检查：Attention Is All You Need（1706.03762v7）

方法：`pdftotext -raw`（xpdf 4.06，Git for Windows 自带）提取 arXiv PDF 全文，用 `cargo run -p lextrail-dict --release --example coverage -- <词典> <文本>` 统计（字母串，保留词内 `'` 和 `-`；去掉单个字母；行末连字符在合并后的词能整词查到时合并）。

**注意**：`pdftotext` 默认模式会把行末连字符直接去掉合并（`attention-` + `based` → `attentionbased`），第一次统计因此出现了 attentionbased、sequencealigned、positionwise、sourcetarget、Englishto-German；`-raw` 和 `-layout` 模式保留连字符。这是提取工具的行为，不是 LexTrail 取词的问题：Edge / Chrome PDF（PDFium）在行末断词处给出标记 `\u{2}`，由 `resolve_line_end_hyphens` 按整词查询决定是否合并；在接入词典之前，`clean_control_chars` 把它一律还原为 `-`。其他 PDF 阅读器（Adobe Acrobat、SumatraPDF 等）的行为还没有测过。

| 项（`-raw` 文本） | 不同的词 | 出现次数 |
|---|---|---|
| 全部 | 1,553 | 5,321 |
| 直接查到 | 884 | 3,755 |
| 经 `exchange` 词形查到 | 385 | 1,003 |
| 经后缀规则查到（embeddings → embedding、subspaces → subspace 等） | 4 | 11 |
| 带连字符、整体查不到的复合词 | 61（其中 56 个各部分都能查到） | — |
| 查不到 | 219 | 406 |

查不到的 219 个词分类：
- **人名、机构、会议、产品名**（约 150 个）：Vaswani、Shazeer、Bengio、Google、NVIDIA、WMT、ACL、arXiv、ByteNet、Xception 等。
- **缩写**：GPU(s)、LSTM、RNN(s)、ReLU、FFN、PCFG、TFLOPS 等。
- **公式变量和 PDF 提取残片**：dmodel、dv、dff、logk、headi、Pdrop、QK、ϵls，以及邮箱 / 网址片段。
- **行末断词没合并的残片**：convolu、tional（convolutional 不在精简版，见 8.1）。
- **普通词汇缺口**：convolutional（7 次）、softmax（6）、codebase（3）、warmup（3）、hyperparameters（2）、parallelization（2）、optimizer（2）、treebank（2）、overfitting、parallelizable、subword、depthwise、params（各 1）。

这些词在完整版 ECDICT（77 万）中的情况：

| 词 | 完整版 | 说明 |
|---|---|---|
| convolutional | 有，被 B+ 过滤 | 没有音标，也没有词频 / 考试 / 柯林斯 / 牛津标记；释义 `[计] 卷积的 / [医] 脑回的` |
| parallelization | 有，被 B+ 过滤 | 同上；释义"并行化" |
| optimizer | 有，被 B+ 过滤 | 同上；释义 `[计] 优化程序 / [化] 最优控制器` |
| overfitting | 有，被 B+ 过滤 | 同上；释义"过适；[数] 过度拟合"（overfit 也被过滤，"[网络] 过拟合"） |
| subword | 有，被 B+ 过滤 | 同上；释义 `[计] 子字` |
| GPU | 有，被 B+ 过滤 | 同上；释义是"地面动力装置（ground power unit）"，不是图形处理器 |
| softmax、codebase、hyperparameter(s)、parallelizable、treebank、depthwise、LSTM、RNN、ReLU | 完整版也没有 | — |
| warmup | 完整版也没有 | 但 warm-up（热身、预热）在精简版中 |
| parallelisation、optimiser | 完整版也没有 | 英式拼写 |

`exchange` 还原结果抽查都正确：best / better → good、worse → bad、is / was / were → be、has → have、could → can、found → find、left → leave、took → take、further → far。

## 10. 待决定：收录规则放宽、行末连字符（2026-10-20 数据）

### 10.1 放宽单词收录规则

假设在 B+ 之外，再收录"单个单词（不含空格）、有中文释义、释义不以领域标记开头"的词条（不要求音标和标记）：

| 项 | B+（现在） | 放宽后 | 增加 |
|---|---|---|---|
| 词条 | 277,936 | 374,824 | 96,888 |
| 义项 | 530,878 | 692,942 | 162,064 |
| 文件 | 33.4 MiB | 43.2 MiB | 9.8 MiB |

新增的 96,888 条中：36% 的释义本身就是"xx 的复数 / 过去式 / 变形"（这些词靠词形还原本来就能查到原形）；8% 是人名、地名；14,694 条带连字符；9,407 条首字母大写。

第 9 节的普通词汇缺口放宽后只多收 parallelization、overfitting（以及释义错误的 GPU）；convolutional、optimizer、subword、overfit 的释义以 `[计]`、`[网络]` 开头，**放宽后仍然收不到**。

随机抽 50 条（种子 20261020）的质量：

| 判断 | 条数 | 例子 |
|---|---|---|
| 明显错误 | 3（6%） | badgered "贪睡的獾（电影名）"（实为 badger 的过去式"纠缠"）；convented "(convent 的过去时) …召集"（convent 不是动词）；fiers "(fier 的复数) 菲尔（人名）" |
| 不完整或过时（像 GPU 那样） | 3（6%） | MRA 只有"道德重整运动"；ecstacy（拼写错误）只有"迷幻药"；conamore 词性标成 v. |
| 价值很低 | 12（24%） | 人名 arrigo、blaisdell、sadowski；没有实际释义的 glossarially、troublousness"xx 的变形"；德语 drueckelement；生僻缩写 ijt、woh；拼写可疑 tilobitus 等 |
| 正常 | 32（64%） | antidromy 反旋、ontologies 本体论、palichnology 古遗迹学、voltaite 绿钾铁矾、swell-mobsman 等，多为生僻专业词 |

### 10.2 行末连字符：精简版查不到的词无法合并

`pdftotext -raw` 提取 1706.03762v7 全文，共 16 处行末断词（行尾"字母-"、下一行以字母开头）。正确答案按原文判断（BerkeleyParser 正文写作一个词）：

| 断词 | 正确 | 现在（精简版整词） | A（完整版词头） | B（片段不是词就合并） |
|---|---|---|---|---|
| English- / to-German | 保留 | 保留 | 保留 | 保留 |
| transduc- / tion | transduction | ✓ | ✓ | ✓ |
| sequence- / aligned（2 处） | 保留 | ✓ | ✓ | ✓ |
| position- / wise | 保留 | ✓ | ✓ | ✓ |
| multi- / plicative | multiplicative | ✓ | ✓ | ✓ |
| transfor- / mation | transformation | ✓ | ✓ | ✓ |
| convolu- / tional | convolutional | ✗ 保留 | ✓ | ✓ |
| source- / target | 保留 | ✓ | ✓ | ✓ |
| sur- / prisingly | surprisingly | ✓ | ✓ | ✓ |
| Berkeley- / Parser | BerkeleyParser | ✗ 保留 | ✗ 保留 | ✗ 保留 |
| Convolu- / tional | Convolutional | ✗ 保留 | ✓ | ✓ |
| im- / age | image | ✓ | ✓ | ✓ |
| Ko- / ray（人名） | Koray | ✗ 保留 | ✗ 保留 | ✗ 保留 |
| attention- / based | 保留 | ✓ | ✓ | ✓ |
| Salakhutdi- / nov（人名） | Salakhutdinov | ✗ 保留 | ✗ 保留 | ✓ |
| **正确数（共 16）** | | **11** | **13** | **14** |

**A：额外存完整版 ECDICT 的单词词头**（只用于判断是否合并，查释义仍只用精简词典）。只需存"完整版有、精简版没有"的纯字母单词（合并结果不含连字符）：165,001 个，小写存放；SQLite 表约 **2.2 MiB**（纯文本 1.6 MiB，gzip 0.5 MiB）。
- 修好：完整版有的词（convolutional）。
- 仍然不合并：完整版也没有的词（人名 Koray、Salakhutdinov；softmax、hyperparameters 等），结果是保留连字符（"hyper-parameters"），可读但不完美。
- 不会误合并真正的复合词。

**B：合并后的词查不到时，只要任一片段本身不是单词就合并。**
- 修好：convolutional、人名 Salakhutdinov。
- 片段"是单词"的判断被词典里的词缀和短词条削弱：tion、im、sur、nov、Ko 都能查到，所以 Ko-ray 仍不合并。
- **会误合并**：片段是缩写、模型名、人名的真复合词。实测这些片段都不在精简版里：COVID、LSTM、RNN、ReLU、softmax、ResNet、BiLSTM、Thang。所以 COVID-related、LSTM-based、RNN-based、ReLU-activated、softmax-normalized、ResNet-based、Minh-Thang 正好断在行末时，会被合并成 COVIDrelated、LSTMbased 等不存在的词——这类复合词在技术文章中很常见，而且误合并后查词和发给 AI 的上下文都会出错。
- （GPT-based、BERT-style、CNN-based 不会误合并：GPT、BERT、CNN 在词典里。）

两个方案共同的局限：合并后恰好是词、但原文的连字符是真的（re-cover、co-op）时都会误合并，与现在相同，很少见。
