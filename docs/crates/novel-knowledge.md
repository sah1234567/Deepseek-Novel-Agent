# novel-knowledge — 知识库系统

> 所属项目: [Novel Agent](../../README.md)

---

## 1. 业务逻辑

知识库用结构化 Markdown 落盘 + 演变日志，替代传统 IDE 索引。

### 1.1 KnowledgeStore

管理作品根目录下的 `knowledge/`、`chapters/` 等路径：
- `read` / `write` / `read_file` / `character_path`
- 通用 Markdown 读写，**无**按文件类型的 Rust 枚举分支

常见 Markdown 路径（由 scaffold 模板与 Agent 维护，非 crate 内硬编码类型）：

| 类别 | 典型路径 |
|------|----------|
| 人物 | `knowledge/characters/*.md` |
| 情节 | `knowledge/plot/`（大纲、细纲、伏笔、因果链） |
| 共享设定 | `knowledge/shared-systems/`（世界观、时间线、场景/道具/势力追踪表等） |
| 多世界 | `knowledge/worlds/<名>/`（Skill 驱动创建） |
| 审计台账 | `knowledge/meta/audit-status.md`（PA/KA/CCA 状态表；**不**写入 `INDEX.md`，`rebuild_index` 会覆写 INDEX） |

场景/道具/势力等追踪表以 **Markdown 表格** 存在于 `shared-systems/`；查询由 **`TrackingQuery` 工具**（`novel-tools`）解析，不再单独 Rust 模块。

### 1.1.1 审计台账（`audit_status.rs`）

- `ensure_audit_status` — 缺失时从 `templates/knowledge/meta/audit-status.md` 创建（迁移-on-read）
- `mark_audited` — Subagent 完成时引擎写入 `已审计`
- `query_summary` / `query_chapter` / `list_pending` — 供 `AuditStatusQuery` 与 `format_progress_hint`（system Progress 段）

**状态五枚举语义（`| 章 | 细纲PA | 正文KA | 文笔CCA | 最后更新 | 备注 |`）：**

| 状态 | 写入方 | 语义 |
|------|--------|------|
| `未审` | 引擎（新行默认） | 尚未审计 |
| `已审计` | 引擎 `mark_audited`（子 Agent 报告注入后自动） | 审计已跑完，**不等于通过**；会出现在 `AuditStatusQuery(pending)` 中 |
| `已通过` | 主 Agent 修复后 `AuditStatusUpdate` / Edit | 审计闭环完成；`query_summary` 的「已通过至 ChN」只统计此状态 |
| `待复审` | 主 Agent | 需要重新审计（如作者改稿后） |
| `不适用` | 主 Agent | 跳过审计；不计入 pending |

注意：`已审计` 未修复的章节仍会被 pending 列出——这是设计（已审计≠已通过）。

### 1.1.2 知识契约与所有权约定（knowledge-contract）

各知识文件的结构契约与所有权约定。契约表与实现代码逐字对齐（代码为 SSOT；改代码先改文档）。

**伏笔追踪表（`knowledge/plot/伏笔追踪.md`）**——首张 markdown 表为数据表，表头 8 列 `| 章节 | 伏笔ID | 操作 | 内容描述 | 状态 | 预计回收章 | 关联人物 |`（单元格索引：章节=1、伏笔ID=2、状态=5、预计回收章=6、关联人物=7；行内 <8 单元格被跳过）。**状态三枚举（子串匹配）**：`待回收` → pending；`已回收` / `已废弃` → 移出 pending。解析只扫首张表，遇下一个 `## ` 或 EOF 停止；追加式维护，禁止覆写旧行。解析实现在 `foreshadow.rs`（`parse_pending_foreshadows` / `categorize_foreshadows` / `build_foreshadow_output`），`ForeshadowTracker` 工具（novel-tools）与 Progress 注入共用。

**人物卡（`knowledge/characters/*.md`）**——frontmatter 必含 `name`（与文件名 stem 一致）；`## 当前状态快照` 为派生段（`derive_character_snapshot` 重建，禁止手工维护）；演变日志表（身份/修为/性格/出场记录）首列章节号；文件名 `_` 开头不视为人物卡。`_关系与称呼索引.md` 为派生文件，由 `derive_relation_cross_index` 重建，禁止手工 Edit。

**追踪表（`knowledge/shared-systems/`）**——FILE_MAP（`tracking_query.rs`）：scene→`场景追踪.md`、prop→`道具追踪.md`、faction→`势力追踪.md`、timeline→`时间线.md`、power→`战力系统.md`、ability→`功法技能.md`；每表在 `## {X}演变日志` 小节下，末行为当前状态，追加式维护。

**章节文件（`chapters/chapter-NNN.md`）**——NNN 3 位补零连续无跳号；正文约束（反 AI 味七项，参数见 `skills/audit-craft/SKILL.md`）：`然后` ≤3 次/章、`不是…(而)是…` 禁用、破折号 ≤1 次/章、禁止结构化序号与 Markdown 标记；字数 2000–4000。**版本化约定：** REGATE 修复前把当前版本快照到 `knowledge/meta/versions/chapter-NNN-{ts}.md`（目录已 gitignore），修复后用 `ChapterDiff` 工具对比新旧两版确认修改范围。

**Ownership 表：**

| 目录/文件 | 所有者 |
|-----------|--------|
| `knowledge/meta/graph-state.json`、`plan-graph.json` | tools-only（GraphTracker/PlanBuilder 读写，Agent 禁止直接 Edit） |
| `knowledge/INDEX.md`、`_关系与称呼索引.md`、人物卡 `## 当前状态快照` | derived（KnowledgeDerive 重建；直接 Edit → WARNING 改用 KnowledgeDerive） |
| 伏笔追踪 / 因果链 / 各演变日志表 / `audit-status.md` | append-only（只追加，不覆写历史） |
| `knowledge/meta/audits/`、`knowledge/meta/findings/` | append-only（见下） |

**审计报告落盘约定**——审计完成后报告全文 Write 到 `knowledge/meta/audits/chapter-NNN-{pa|ka|cca}.md`（多章按章归档；重跑覆盖）；每条问题标注 `[可泛化]`（值得沉淀为规则的错误模式）或 `[一次性]`。

**findings 落盘与闭环**——`knowledge/meta/findings/chapter-NNN.md` 追加式，条目 = {类型, 章节, 原文证据, 修复动作, 可泛化}。消费：每 10 章一次（`AuditStatusUpdate` 通过至 N%10==0 时）由 GeneralPurpose 提取器扫描 `extract_generalizable_findings`（本 crate），重复 ≥2 次的 `[可泛化]` 模式沉淀为 `memory/rejected_paths/` 或 `memory/style/` 记忆，并标注「已沉淀」避免重复沉淀。

### 1.2 Frontmatter

`parse_frontmatter<T>` — YAML + Markdown body

CharacterFrontmatter：name, aliases, category, first_appearance, last_update, status, pov_character

### 1.3 演变日志

**append_evolution_log：** 定位表格 → 末行后追加（append-only）；续写通过 Grep/Read 末行或工具层查询，不在磁盘上合并历史行

### 1.4 因果链

`CausalityGraph`（petgraph）：add_edge（环检测）、traverse_forward/backward、parse_causality_markdown

### 1.5 Rust 子模块

| 模块 | 职责 |
|------|------|
| `character` | 人物 frontmatter 类型 |
| `causality` | 因果图（add_edge 环检测、traverse、断头边查询） |
| `evolution_log` | 演变日志 append、末行查询 |
| `foreshadow` | 伏笔追踪表解析 + 分类输出 + digest（§1.1.2 契约的实现） |
| `findings` | 审计发现 `[可泛化]` 提取（闭环数据源） |
| `index` | `rebuild_index`, `ensure_index` |
| `derive` | 派生快照（纯函数，不自动写盘） |
| `scaffold` / `scaffold_templates` | 新建作品目录树 |
| `project_tree` | IPC 文件树列表 |
| `parser` | YAML frontmatter |

已移除的专用模块：`scene`、`prop`、`faction`、`skill_encyclopedia`、`regex_cache`（逻辑合并为通用 Markdown + 工具层）。

### 1.6 derive — 派生快照

纯函数，不自动写盘：
- `derive_character_snapshot` — 人物卡「当前状态快照」重建
- `derive_foreshadow_categories` — 伏笔按 pending/resolved/abandoned 分组
- `derive_relation_cross_index` — `_关系与称呼索引.md` 重建
- `derive_foreshadow_digest`（foreshadow.rs）— ≤500 字活跃伏笔摘要（Progress 注入）
- `derive_work_digest` — 剧情状态快照：最近章节事件 + 人物快照 + 伏笔摘要（Progress 注入；空作品返回 None 不产生噪音）

由 Agent、**KnowledgeDerive** 工具调用，或由 novel-core 的 `load_progress`（Progress 段）直接调用（digest 两个函数）。

### 1.7 scaffold — 项目初始化

`init_project_scaffold(project_root, templates_dir)` — 目录树 + 必建模板（**仅单世界骨架**，不创建 `knowledge/worlds/`）。模板**必须**存在于 `{agent_root}/templates/**/*.md`；缺失或为空时返回 `TemplatesNotFound` 错误。

Tauri `init_novel_project` / `create_work` / 启动时 `works/default` → 调用 scaffold；`project_initialized` 反映 knowledge 目录是否存在。

**模板来源 `{agent_root}/templates/`（walkdir 递归）：** 文件须落在 `knowledge/`、`memory/`、`chapters/`（目录由 `SCAFFOLD_DIRS` 创建）、`AGENTS.md`、`plan/` 等作品根下路径。**勿**在 `templates/` 下放顶层 `plot/` 或 `shared-systems/`（会复制到错误路径；剧情/设定模板只用 `templates/knowledge/plot/`、`templates/knowledge/shared-systems/`）。典型文件：`AGENTS.md`、`knowledge/characters/_template.md`、`knowledge/plot/*.md`、`memory/*.md`。已存在文件不覆盖。

**单世界（默认）：**

```
knowledge/characters/     ← 全部人物
knowledge/shared-systems/ ← 世界观、时间线、场景/道具/势力追踪表等
knowledge/plot/           ← 大纲、伏笔、因果链、细纲
chapters/                 ← 章节正文（扁平，无世界子目录）
memory/                   ← 跨 session 记忆（Agent Write 维护）
```

**不创建** 作品级 `skills/`（Skill 在 agent 根 `skills/`）。

**多世界（Skill 约定，Agent 策划时手动创建）：**

在相同根结构上增加 `knowledge/worlds/<世界名>/`：

| 位置 | 内容 |
|------|------|
| `characters/`（共享层） | 穿越者、主角等跨世界角色 |
| `worlds/<名>/characters/` | 该世界原住民 NPC |
| `shared-systems/`（共享层） | 贯穿多界的设定 |
| `worlds/<名>/world-building/` | 该世界独立背景/势力/时间线 |
| `worlds/<名>/INDEX.md` | 该世界规则摘要 |

`chapters/` 仍扁平；大纲/关系索引建议加「所在世界」列。`rebuild_index` 已支持扫描 `worlds/` 子目录，自动索引各世界的角色与 INDEX 摘要。

### 1.8 INDEX

`knowledge/INDEX.md` 由 `rebuild_index` 维护，注入 system prompt 与 compaction refresh。当前扫描 `characters/`、`plot/`、`shared-systems/`，以及 `worlds/` 子目录下的所有世界。

### 1.9 记忆类型（Memory Types）

全部记忆逻辑（类型定义、文件扫描、选择、提取、预取、fork 守卫）在 [`novel-memory`](novel-memory.md) 中自包含，**不依赖** `novel-knowledge`。此前共用的三个组件（`parse_frontmatter`、`truncate_bytes_utf8`、`KnowledgeStore`）已内联或替换为 `std::fs`。
