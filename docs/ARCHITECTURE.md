# LexTrail 架构设计

> 本文件描述 LexTrail 的整体架构。每项设计背后的理由见 `DECISIONS.md`，文中以 `Dxxx` 标注对应的决策编号。
>
> 标注为 **〔初稿〕** 的部分是讨论中尚未逐项确认的技术细节，开发到对应里程碑时应先评审再实现，评审结果写回本文件和 `DECISIONS.md`。

---

## 1. 产品概述

LexTrail 是一款常驻后台的桌面翻译与词汇学习工具：

- 在**任何软件**中选中英文单词、短语或句子，按快捷键即可在鼠标旁弹出翻译。
- 显示音标、词性、按常用程度排序的释义，以及 **AI 给出的当前语境含义**、例句和翻译。
- 一键收藏到生词本，自动保存遇见时的原句和来源。
- 使用 FSRS 算法复习，并可在手机上通过 PWA 离线复习，多设备自动同步。

---

## 2. 架构总览

```
┌─────────────────────────── 桌面端（Tauri 2）───────────────────────────┐
│                                                                         │
│  输入来源：快捷键 │ 连按两次 Ctrl+C │ 截图 OCR │ 阅读模式(1.1) │ 浏览器扩展(以后) │
│        │                                                                │
│        ▼                                                                │
│  lextrail-capture   取词层（按平台实现：Windows 现在做，macOS 以后）     │
│        │                                                                │
│        ▼                                                                │
│  查询引擎                                                               │
│   ├─ 输入分类：单词 / 短语 / 句子                                       │
│   ├─ lextrail-dict  词形还原、本地词典查询 ──▶ 立即显示                 │
│   ├─ lextrail-ai    AI 适配器（多服务商）  ──▶ 流式补充                 │
│   └─ 缓存                                                               │
│        │                                                                │
│        ▼                                                                │
│  lextrail-core  数据模型、op-log、FSRS、同步合并逻辑                    │
│        │                                  │                             │
│        ▼                                  ▼                             │
│  本地 SQLite                       同步客户端 ─────────────┐            │
│                                                            │            │
│  界面：悬浮窗（轻量页面）│ 主窗口（生词本/复习/设置）│ 托盘 │            │
└────────────────────────────────────────────────────────────┼────────────┘
                                                             │ HTTPS
                                          ┌──────────────────▼──────────┐
                                          │ lextrail-server（AGPL）      │
                                          │  多用户、Token 认证、op-log  │
                                          │  同时提供手机端 PWA          │
                                          └──────────────────┬──────────┘
                                                             │
                                          ┌──────────────────▼──────────┐
                                          │ 手机端 PWA（只做复习，可离线）│
                                          │  lextrail-core 编译为 WASM   │
                                          └─────────────────────────────┘
```

**核心原则**（D006、D007）：
1. 业务逻辑全部在 Rust crate 中，界面层很薄。
2. 同步、AI、取词平台都是"统一接口 + 可替换适配器"。

---

## 3. 仓库结构

单一仓库（monorepo）：Rust 部分是一个 Cargo workspace（根 `Cargo.toml`，成员为 `crates/*`、`apps/desktop/src-tauri`、`tools/bench`），前端部分是一个 pnpm workspace（`pnpm-workspace.yaml`，成员为 `apps/desktop`、`apps/pwa`、`packages/*`）。`tools/dict-builder` 在 M1 加入 Cargo workspace；`apps/server` 是否加入在 M4 决定。M0 评审时已对照实际代码更新（2026-10-15）；标注"占位"的目录目前只有 README 或空的 `lib.rs`，在对应里程碑实现。

```
lextrail/
├── AGENTS.md                 AI 协作入口
├── CLAUDE.md                 指向 AGENTS.md
├── README.md
├── LICENSE                   MIT（客户端及共用模块）
├── Cargo.toml                Rust workspace；共用的 package 字段与 lint 规则
├── rust-toolchain.toml       固定 Rust 工具链
├── package.json              根脚本（pnpm check / lint / format / test）与共用开发依赖
├── pnpm-workspace.yaml       pnpm workspace
├── .github/workflows/ci.yml  CI：ubuntu 跑格式、ESLint、类型检查；Windows 跑 clippy、测试、tauri build
├── crates/
│   ├── lextrail-core/        数据模型、op-log、合并逻辑、FSRS 封装（可编译为 WASM）（占位，M3）
│   ├── lextrail-dict/        词典访问、词形还原、义项拆分（占位，M1）
│   ├── lextrail-ai/          AI 适配器、提示词模板、分段标记流式解析（占位，M2）
│   ├── lextrail-capture/     取词层（占位，M1；计划按平台分 windows.rs，以后加 macos.rs）
│   ├── lextrail-store/       桌面端本地 SQLite 读写与迁移（D037、D040；core 不依赖 SQLite）
│   └── lextrail-config/      本地设置（TOML）与密钥存储（系统凭据管理器）（D035）
├── apps/
│   ├── desktop/              Tauri 应用（npm 包 @lextrail/desktop）
│   │   ├── src-tauri/        Rust 端（crate lextrail-desktop），只做调用和窗口管理：
│   │   │                       main.rs（启动、插件、单实例）、tray.rs、hotkey.rs、settings.rs、
│   │   │                       notice.rs（原生提示框）、memory.rs（低内存通知）、
│   │   │                       popup/（悬浮窗：lifecycle 状态机、placement 定位、webview 挂起与恢复）、
│   │   │                       main_window.rs（主窗口，关闭即销毁）
│   │   └── src/              Vue 前端：popup/（悬浮窗，独立轻量入口）、main/（主窗口）；两个 Vite 入口
│   ├── server/               同步服务端（占位，M4；LICENSE：AGPL-3.0）
│   └── pwa/                  手机端复习 PWA（占位，M4；构建后嵌入服务端）
├── packages/
│   └── ui/                   桌面端主窗口与 PWA 共用的 Vue 组件（复习卡片等）（占位，M3）
├── tools/
│   ├── dict-builder/         ECDICT 预处理：生成精简词典、拆分义项（占位，M1；Rust，crate lextrail-dict-builder，D038）
│   └── bench/                性能测试工具（crate lextrail-bench，仅 Windows），见 docs/specs/m0-benchmark.md
└── docs/
    ├── STATUS.md             进度交接单
    ├── ROADMAP.md            里程碑与任务清单
    ├── DECISIONS.md          决策记录
    ├── ARCHITECTURE.md       本文件
    └── specs/                各模块的详细规格（按里程碑补充）
```

**命名**（D001）：crates.io、PyPI 上已有名为 `lextrail` 的包，因此：
- Rust crate 一律用 `lextrail-<模块>`（如 `lextrail-core`、`lextrail-desktop`、`lextrail-bench`）；
- npm 包一律用 `@lextrail/<模块>` 作用域（如 `@lextrail/desktop`）；根 `package.json` 名为 `lextrail-monorepo`，`private`，不发布；
- 所有包目前都不发布（`publish = false` / `private`）。

**许可证**：workspace 默认 `license = "MIT"`；`apps/server` 使用 AGPL-3.0，带独立的 `LICENSE` 文件（D026）。具体写法（only / or-later）在 M4 决定。

**词典数据**（D038）：ECDICT 原始数据和生成的词典不提交到仓库；构建时由 `dict-builder` 下载固定版本并校验 SHA-256，CI 缓存。

---

## 4. 取词层（lextrail-capture）

### 4.1 统一接口

```rust
pub struct Selection {
    pub text: String,
    pub context: Option<String>,    // 所在句子或段落
    pub source_app: Option<String>, // 来源软件
    pub url: Option<String>,        // 浏览器扩展可提供
    pub method: CaptureMethod,      // Uia / Clipboard / ClipboardDoubleCopy / Ocr
}

pub trait TextCapture {
    fn get_selection(&self) -> Result<Selection>;
    fn ocr_region(&self, rect: Rect) -> Result<String>;
}
```

### 4.2 Windows 实现（D011、D012）

| 步骤 | 方式 | 要点 |
|---|---|---|
| 1 | UI Automation | 读取选中文本，尽量读取所在句子；不污染剪贴板 |
| 2 | 剪贴板兜底 | 等待修饰键松开 → 模拟 Ctrl+C（终端用 Ctrl+Insert）→ 用剪贴板序列号确认已更新 → 读取 → 恢复原内容 |
| 3 | OCR | `Windows.Media.Ocr`，用户框选区域 |
| 另 | 连按两次 Ctrl+C | 监听剪贴板变化通知；< 约 50 ms 的连续更新算同一次复制；50–500 ms 内第二次更新判定为连按；触发后尝试用 UIA 补充上下文 |

已知限制：无法读取以管理员权限运行的程序（除非本软件也以管理员身份运行）。连按两次 Ctrl+C 不受此限制。

---

## 5. 查询引擎

### 5.1 查询流程

```
收到 Selection
  ├─ 分类：单词 / 短语 / 句子
  ├─ 单词或短语：
  │    ├─ 词形还原（ECDICT exchange 字段）
  │    ├─ 查本地词典 ──▶ 立即推送给悬浮窗（音标、词性、释义、考试标签）
  │    └─ 查 AI 缓存，未命中则请求 AI（附带词典释义 + 上下文）──▶ 流式推送
  ├─ 句子：
  │    └─ 请求 AI 翻译，标出难词（点击难词可继续查）
  └─ 写入查询历史
```

### 5.2 词典（lextrail-dict，D015、D019、D021）

- 数据源：ECDICT（MIT）。
- `tools/dict-builder`（Rust，D038）负责预处理；构建时下载固定版本的 ECDICT 并校验哈希，数据不提交到仓库：
  - 生成**精简版**核心词典（约十几万词条，只保留需要的字段），打包进安装包；
  - 把中文释义拆分为独立义项（注意：逗号分隔的近义说法属于同一义项）；
  - 建立索引，查询时按需从磁盘读取，不整体载入内存。
- 完整词典作为可选数据包下载。

### 5.3 AI 层（lextrail-ai，D016–D018）

**适配器接口**：

```rust
pub trait AiProvider {
    fn id(&self) -> &str;
    async fn stream(&self, req: AiRequest) -> Result<impl Stream<Item = Result<String>>>;
    async fn test_connection(&self) -> Result<()>;
}
```

- 第一个实现：**OpenAI 兼容格式**（`base_url` + `api_key` + `model`），覆盖大多数服务商和 Ollama。
- 以后：Claude、Gemini 原生适配器；"LexTrail 官方"适配器（官方免费额度）。

**功能**：多服务商配置；按任务分配模型（查词 / 句子翻译）；悬浮窗一键换模型重新解析；超时或出错自动切换备用模型；内置默认提示词，高级设置中可编辑；API Key 存入系统凭据管理器。

**提示词要点**：
- 输入：选中的词、所在句子、**词典释义列表**。
- 要求：判断当前语境下的含义（必须从词典释义中选，词典中没有时才自行解释）；给出语境解释、例句；把其余义项按常用程度分为三档（D019）。
- 不要求 AI 输出音标和词性（以词典为准）。

**输出格式：分段标记**（D017）〔初稿，标记名称待定〕：

```
【语境】v. 处理，解决（问题）
【解释】在这句话里 address 指着手处理某个问题……
【例句】We must address the root cause. | 我们必须解决根本原因。
【排序】n. 地址=常用; n. 演讲=较常用; v. 写地址=少见; v. 称呼=少见
```

客户端边接收边解析：遇到新的段落标记就开始渲染该段，格式偏差时尽量容错。

**缓存**：
- 语境解析：按（服务商、模型、单词、上下文哈希）缓存。
- 词义排序：按（服务商、模型、单词）缓存，与上下文无关。

---

## 6. 数据模型

> 评审状态：公共字段、HLC 时间戳和 `lookups` 表已在 M1 开始时评审确定（D039）；其余表在 M3 评审，仍为初稿。本地 SQLite 的读写与迁移在 `lextrail-store`（D037），`lextrail-core` 不依赖 SQLite。

### 6.0 本地数据库（D037、D040）

- **位置**：`%LOCALAPPDATA%\io.github.aspeternity.lextrail\lextrail.db`（Tauri 的 `app_local_data_dir()`）。不放在 Roaming（`%APPDATA%`）：域环境下 Roaming 目录会在多台电脑间漫游，导致多台设备共用同一个数据库和 `device_id`。设置文件仍在 `%APPDATA%`（D035）。
- **按需打开**：只有功能第一次需要时才打开（例如第一次写查询历史）；只有托盘待机时不打开数据库。
- **驱动**：`rusqlite`（`bundled`，自带 SQLite，不依赖系统版本）。表使用 `STRICT`，类型不符直接报错。
- **连接设置**：`journal_mode = WAL`、`synchronous = NORMAL`、`foreign_keys = ON`、`busy_timeout = 5000` 毫秒。
- **迁移**：用 SQLite 的 `user_version` 记录结构版本；迁移脚本只追加、发布后不修改，每个迁移与版本号更新放在同一事务中。打开时发现数据库版本**高于**本程序支持的版本，直接拒绝打开，**不做任何修改**（检查在切换 WAL 之前进行）。
- **HLC 持久化**：每次写入时，`meta` 表中的"最近一次 HLC"与数据在同一事务中更新；事务失败时内存中的时钟一并回滚。
- **测试**：内存数据库；从版本 0 逐级迁移到最新，结果与直接迁移一致；更高版本数据库被拒绝且文件不变。

### 6.1 同步元数据公共字段（D024、D039）

所有可同步的表都带以下字段：

| 字段 | 类型 | 说明 |
|---|---|---|
| `id` | TEXT | UUIDv7，小写、带连字符的标准格式。用文本是为了与 PWA 的 IndexedDB、JSON 保持一致，也便于调试 |
| `created_at` | INTEGER | 本机系统时间（Unix 毫秒）。只用于显示和排序，**不参与冲突判断** |
| `updated_at` | TEXT | HLC 时间戳（见 6.2）。"最后修改者优先"只比较这个字段 |
| `device_id` | TEXT | 最后修改这条记录的设备 |
| `deleted` | INTEGER | 软删除标记，`0` / `1`。已删除的记录暂时永久保留，清理规则在 M4 确定 |

- **本地表不带 `user_id`**：用户可以先离线使用、以后再登录，本地记录带 `user_id` 会导致登录时要改写全部记录。服务端按登录账号区分用户，服务端的表保留 `user_id`。
- **`device_id`**：第一次打开数据库时生成一个 UUIDv7，保存在 `lextrail-store` 的本地 `meta` 表中（不同步），不放在 `settings.toml`，这样只拷贝设置文件不会让两台设备共用同一个 ID。
- **合并粒度**：按整行合并，`updated_at` 大的一行整体胜出。按字段合并留到 M3 评审 `vocab_items` 时再考虑。

### 6.2 HLC 时间戳（D039）

**格式**：固定宽度文本 `<物理毫秒>-<计数器>-<device_id>`

| 部分 | 宽度 | 说明 |
|---|---|---|
| 物理毫秒 | 12 位十六进制 | Unix 毫秒，左侧补 0 |
| 计数器 | 4 位十六进制 | 同一毫秒内的序号，0–`ffff` |
| `device_id` | 36 字符 | 产生该时间戳的设备，UUID 标准格式 |

示例：`019a1b2c3d4e-0000-019a1b2c-3d4e-7abc-8def-0123456789ab`

- 十六进制**统一使用小写**。解析时严格校验：长度、分隔符位置、字符集（只允许 `0-9a-f`），不符合的一律拒绝，不做大小写转换。
- 按字符串比较即为时间先后，SQLite、IndexedDB、JSON 中行为一致；不用 64 位整数，避免 JS 数字超过 2^53 时丢失精度。
- 时间戳中带有 `device_id`，不同设备的时间戳不会相等。

**时钟规则**（标准 HLC，纯逻辑，放在 `lextrail-core`，WASM 也能用）：

- 本机产生新时间戳：物理部分取 max(系统时间, 上次物理部分)；与上次相同时计数器 +1，否则计数器归 0。
- 收到其他设备的时间戳：本机时钟推进到同时大于本机上次值和对方值。
- **计数器溢出**：计数器已是 `ffff` 还需要 +1 时，物理部分 +1 ms、计数器归 0。
- 最近一次的 HLC 保存在 `meta` 表中，系统时间往回调或重启后，时间戳仍然只增不减。
- 必须有单元测试：格式化与解析往返、大写和非法字符被拒绝、单调递增、系统时间回拨、接收远端时间戳、计数器溢出。
- 对方时钟快得离谱时的处理（超过某个阈值时拒绝还是只记警告）在 M4 确定。

### 6.3 `lookups`（查询历史，D039）

```sql
-- 实际定义见 crates/lextrail-store/src/migrations/0001_init.sql（表为 STRICT）
CREATE TABLE lookups (
  id          TEXT    PRIMARY KEY,
  created_at  INTEGER NOT NULL,
  updated_at  TEXT    NOT NULL,
  device_id   TEXT    NOT NULL,
  deleted     INTEGER NOT NULL DEFAULT 0 CHECK (deleted IN (0, 1)),
  query       TEXT    NOT NULL,  -- 选中的文字，合并连续空白，最多 2000 字符
  kind        TEXT    NOT NULL CHECK (kind IN ('word', 'phrase', 'sentence')),
  lemma       TEXT,              -- 词形还原后的原形；句子或词典中查不到时为 NULL
  context     TEXT,              -- 所在句子，最多 2000 字符
  source_app  TEXT,              -- 进程名，如 chrome.exe
  url         TEXT,              -- 预留，浏览器扩展提供
  method      TEXT    NOT NULL CHECK (method IN ('uia', 'clipboard', 'clipboard_double_copy', 'ocr'))
);
CREATE INDEX lookups_lemma_created ON lookups (lemma, created_at);
CREATE INDEX lookups_created ON lookups (created_at);
```

- **只新增、只软删除，不修改**（与 `review_logs` 一样，合并不会冲突）。AI 结果（使用的模型、语境含义）与查询记录的关联在 M2/M3 设计"你见过 N 次"时确定，通过迁移增加字段或表。
- **不保存窗口标题**：窗口标题常含邮件主题、文件名等隐私内容，而查询历史会同步到服务器。取词时仍可读取窗口标题（例如判断是否为终端），但不写入数据库。
- **`url`**：接入浏览器扩展时再评审隐私处理（例如是否只保存域名、是否允许关闭）。
- **去重**：同一 `query` + 同一 `context` 在 30 秒内重复查询只记一次（未删除的记录；按时间差的绝对值判断，系统时间回拨不会误判）。去重逻辑放在 `lextrail-core` / `lextrail-store`，不放在界面层。
- **"不记录查询历史"开关**：打开后不写入 `lookups`。应用黑名单中的程序本来就不会产生查询（D032）。
- **`lemma` 依赖词典版本**：词典或词形还原规则更新后，旧记录的 `lemma` 可能与新结果不一致。以后做"查过 N 次"等统计时，可能需要按当前词典重新归一化（重新计算或在查询时归一）。

### 6.4 其余表〔初稿，M3 评审〕

| 表 | 是否同步 | 内容 |
|---|---|---|
| `vocab_items` | 是 | 收藏的词条：lemma、`type`（word/phrase）、词典快照、AI 解析快照（含例句）、标签 |
| `encounters` | 是 | 遇见记录：词条 ID、原句、来源软件、网址、当时的语境含义 |
| `review_logs` | 是 | 复习日志，**只追加** |
| `cards` | 由日志计算 | FSRS 状态（稳定性、难度、下次复习时间），可在本地缓存，但以 `review_logs` 为准 |
| `sentences` | 是 | 句子收藏（预留，第一版不做界面） |
| `ai_cache` | 否 | AI 结果缓存，仅本地 |
| `outbox` | 否 | 待上传的 op-log 记录 |
| `meta` | 否 | 本机键值信息：`device_id`、最近一次 HLC 等 |

### 6.5 待定事项

- **M3**：删除与修改并发时的规则——一台设备删除、另一台设备在之后修改时，记录是否会"复活"。
- **M4**：换账号（登出后登录另一个账号）时，本地已有数据归属哪个账号、如何处理。
- **M4**：重复 `device_id` 的检测（例如整个数据目录被拷贝到另一台电脑），以及检测到后如何重新生成。
- **M4**：远端时钟超前过多时的处理阈值（见 6.2）。
- **M4**：已删除记录的清理规则（见 6.1）。

---

## 7. 同步（D024–D027）

### 7.1 原理
- **本地优先**：客户端保存完整数据；服务器只中转操作日志。
- 每次本地修改生成一条 op（插入、更新、软删除），写入本地数据库的同时写入 `outbox`。
- 服务器为每个用户的 op 分配递增序号；客户端记录自己拉取到的位置（cursor）。
- op 带唯一 ID，重复应用不会产生副作用（幂等）。
- 合并规则：`review_logs` 只追加，天然无冲突；其他记录按"最后修改者优先"。

### 7.2 接口 〔初稿〕

```
POST /api/v1/auth/login           登录，返回 access token + refresh token
POST /api/v1/auth/refresh         刷新 token
POST /api/v1/sync/push            上传一批 op
GET  /api/v1/sync/pull?since=N    拉取序号 N 之后的 op
GET  /api/v1/health               健康检查与服务端版本
```

### 7.3 服务端（apps/server，AGPL-3.0）
- Rust（建议 axum），SQLite 存储，单个可执行文件 + Docker 镜像。
- 多用户架构；自建版默认只开放一个账号（D025）。
- 把构建好的 PWA 嵌入可执行文件中，直接对外提供。
- 官方服务阶段再增加：注册、邮箱验证、找回密码、配额、限流、付费；视规模决定是否换用 PostgreSQL。

### 7.4 同步适配器
- 第一阶段：`HttpServerAdapter`（自建服务器 / 官方服务）。
- 以后：`WebDavAdapter`（D025 中推迟）。

---

## 8. 手机端 PWA（D029–D031）

- 只做复习；由同步服务器提供；可添加到主屏幕。
- **离线复习**：Service Worker 缓存页面；IndexedDB 缓存近期待复习卡片和复习日志；恢复网络后自动同步。
- 复习算法和合并逻辑使用编译为 WebAssembly 的 `lextrail-core`，与桌面端为同一份代码（D023）。
- 复用 `packages/ui` 中的复习组件。
- 复习卡片显示：单词、音标、发音按钮；点击后显示语境含义、用户遇见时的原句及来源、AI 例句；评分按钮：忘记 / 困难 / 良好 / 简单。
- 第一版不做推送提醒。

---

## 9. 悬浮窗

### 9.1 生命周期（D009、D010）

```
显示中 ──关闭/Esc/点击外部──▶ 隐藏（几秒） ──▶ 挂起（TrySuspend，一直保持）
   ▲                                                    │
   └────────────────────── 快捷键（Resume）─────────────┘

只有以下情况才销毁：收到系统低内存通知；用户选择了"内存优先"模式。
```

### 9.2 界面要点
- 在鼠标附近弹出；Esc 或点击外部关闭；可钉住；带收藏和发音按钮。
- 上半部分：词典结果（立即显示）。
- 下半部分：AI 语境解析（流式显示）；未配置 AI 时显示一行灰色提示。
- 词义列表：当前语境含义置顶高亮；其他义项按常用程度排序，标注三档和"你见过 N 次"。
- 单独打包的轻量页面；直接显示，不加淡入动画。

示意：

```
address                         ★  🔊  📌
英 /əˈdres/   美 /ˈædres/
──────────────────────────────────────
▶ 当前语境  v. 处理，解决（问题）
  在这句话里 address 指着手处理某个问题……
  例：We must address the root cause.
──────────────────────────────────────
其他含义
  n. 地址          ●●● 常用
  n. 演讲，致辞    ●●○ 较常用   · 你见过 1 次
  v. 写地址        ●○○ 少见
```

---

## 10. 性能预算（D004、D008、D009）

M0 阶段必须实测以下指标；任何一项不达标，都要在 `DECISIONS.md` 中记录结论并决定是否切换到路线 B。

| 指标 | 目标 |
|---|---|
| 纯托盘状态内存（无 WebView） | < 30 MB |
| 悬浮窗挂起时的总内存（含 WebView2 所有子进程） | < 100 MB |
| 刚挂起后恢复显示的时间 | < 100 ms |
| **挂起 2 小时后**恢复显示的时间 | 待测，目标 < 300 ms |
| 冷启动显示悬浮窗的时间 | < 400 ms |
| 空闲时 CPU 占用 | ≈ 0% |

统计口径（D036）：
- 内存一律按**私有工作集**（Private Working Set，即任务管理器"内存"列）统计，"总内存"指应用进程及其全部 WebView2 子进程之和。私有提交（Private Commit）和工作集只作参考，同时记录在报告中。
- 测量工具：`tools/bench`（`lextrail-bench`）；方法和历次结果见 `docs/specs/m0-benchmark.md`。

其他约束：
- 不轮询；全部事件驱动。
- 只有开启阅读模式时才启用全局鼠标监听（D013）。
- OCR、TTS 等模块按需初始化。
- 不使用 `EmptyWorkingSet` 之类手段"美化"内存数字。

---

## 11. 数据包（D021）

| 数据包 | 默认 | 说明 |
|---|---|---|
| 精简核心词典 | 内置 | 安装包自带 |
| 完整词典 | 可选下载 | ECDICT 完整数据 |
| 词义频率表 | 可选下载 | 第二阶段（D019），约 1 MB |
| 离线翻译模型 | 可选下载 | 待确认可行性 |

数据包有独立的版本号，可以在不发布新版软件的情况下更新。

---

## 12. 主要设置项

| 分类 | 设置 |
|---|---|
| 触发 | 快捷键；连按两次 Ctrl+C（默认关）；阅读模式（1.1） |
| 悬浮窗 | 响应优先 / 内存优先 |
| 词义 | 统一排序 / 按词性分组 |
| AI | 服务商列表、默认模型、按任务分配模型、备用模型、提示词编辑 |
| 同步 | 服务器地址、账号登录、同步状态 |
| 隐私 | 应用黑名单、调试日志开关 |
| 数据包 | 下载与更新 |
| 通用 | 开机启动、界面语言（中文 / English） |

---

## 13. 安全与隐私（D032）

- API Key 与同步 Token 存入系统凭据管理器。
- 首次配置 AI 时明确说明：选中的文字会发送给所选服务商。
- 应用黑名单中的程序不触发查词。
- 默认日志不记录查询内容。

---

## 14. 发布

- GitHub Actions 自动构建、测试、打包（NSIS / MSI 安装包）。
- Tauri updater 通过 GitHub Releases 自动更新。
- 初期不做代码签名（会出现 SmartScreen 警告，需在帮助文档中说明）；用户增多后再考虑购买证书。
- 许可证：根目录 `LICENSE` 为 MIT；`apps/server/LICENSE` 为 AGPL-3.0（D026）。
