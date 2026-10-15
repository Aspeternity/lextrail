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
│   ├── lextrail-store/       桌面端本地 SQLite 读写与迁移（M1 创建，D037；core 不依赖 SQLite）
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

## 6. 数据模型 〔初稿〕

> 评审安排（D039）：同步元数据公共字段、时间戳格式（倾向 HLC）和 `lookups` 表在 M1 开始时评审；其余表在 M3 评审。本地 SQLite 的读写与迁移在 `lextrail-store`（D037），`lextrail-core` 不依赖 SQLite。

所有可同步的表都带有同步元数据（D024）：

| 字段 | 说明 |
|---|---|
| `id` | UUID（建议使用 UUIDv7，按时间有序，便于排序和分页） |
| `user_id` | 本地只有一个用户，但字段保留，与服务端一致 |
| `created_at` / `updated_at` | 建议使用混合逻辑时钟（HLC），避免不同设备的系统时间不一致导致"最后修改者优先"判断错误 〔待评审〕 |
| `device_id` | 最后修改的设备 |
| `deleted` | 软删除标记 |

| 表 | 是否同步 | 内容 |
|---|---|---|
| `vocab_items` | 是 | 收藏的词条：lemma、`type`（word/phrase）、词典快照、AI 解析快照（含例句）、标签 |
| `encounters` | 是 | 遇见记录：词条 ID、原句、来源软件、网址、当时的语境含义 |
| `lookups` | 是 | 查询历史：查询文本、lemma、上下文、来源、使用的模型（用于"查过 N 次"提示和个人词义标注） |
| `review_logs` | 是 | 复习日志，**只追加** |
| `cards` | 由日志计算 | FSRS 状态（稳定性、难度、下次复习时间），可在本地缓存，但以 `review_logs` 为准 |
| `sentences` | 是 | 句子收藏（预留，第一版不做界面） |
| `ai_cache` | 否 | AI 结果缓存，仅本地 |
| `outbox` | 否 | 待上传的 op-log 记录 |

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
