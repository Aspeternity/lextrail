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
- `entries`：查到的词条自己的词条（如果它不只是"xx 的过去式"之类说明），然后是原形的词条。例如 `launched` 只给 `launch`；`better` 给 `better`（更好的、改善）和 `good` 两个。
- `inflection`：原形、词形种类（过去式、复数等）和来源（`exchange` / 后缀规则 / 短语）。
- `lemma()`：写入查询历史 `lookups.lemma` 的原形。

带连字符的复合词（`self-attention`）目前整体查不到就返回 `None`，不拆开查。

## 9. 覆盖率检查：Attention Is All You Need（1706.03762v7）

方法：`pdftotext` 提取 arXiv PDF 全文，用 `cargo run -p lextrail-dict --release --example coverage -- <词典> <文本>` 统计（字母串，保留词内 `'` 和 `-`；去掉单个字母；行末连字符在合并后的词能查到时合并）。

| 项 | 不同的词 | 出现次数 |
|---|---|---|
| 全部 | 1,548 | 5,302 |
| 直接查到 | 881 | 3,738 |
| 经 `exchange` 词形查到 | 384 | 1,001 |
| 经后缀规则查到（embeddings → embedding、subspaces → subspace） | 3 | 9 |
| 带连字符、整体查不到的复合词 | 61（其中 55 个各部分都能查到） | — |
| 查不到 | 219 | 409 |

查不到的 219 个词分类：
- **人名、机构、会议、产品名**（约 150 个）：Vaswani、Shazeer、Bengio、Google、NVIDIA、WMT、ACL、arXiv、ByteNet、Xception 等。
- **缩写**：GPU(s)、LSTM、RNN(s)、ReLU、FFN、PCFG、TFLOPS 等。
- **公式变量和 PDF 提取残片**：dmodel、dv、dff、logk、headi、Pdrop、QK、ϵls，以及邮箱 / 网址片段。
- **PDF 提取时丢了连字符的复合词**：attentionbased、sequencealigned、positionwise、sourcetarget。
- **真正的普通词汇缺口**（ECDICT 没有的学术 / 技术词）：convolutional（9 次）、softmax（7）、codebase（3）、warmup（3）、hyperparameters（2）、parallelization（2）、optimizer（2）、treebank（2）、overfitting、parallelizable、subword、depthwise、params（各 1）。

`exchange` 还原结果抽查都正确：best / better → good、worse → bad、is / was / were → be、has → have、could → can、found → find、left → leave、took → take、further → far。
