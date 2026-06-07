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
| 审计台账 | `knowledge/meta/audit-status.md`（PA/KA/CCA 状态 + 修复记录；**不**写入 `INDEX.md`，`rebuild_index` 会覆写 INDEX） |

场景/道具/势力等追踪表以 **Markdown 表格** 存在于 `shared-systems/`；查询由 **`TrackingQuery` 工具**（`novel-tools`）解析，不再单独 Rust 模块。

### 1.1.1 审计台账（`audit_status.rs`）

- `ensure_audit_status` — 缺失时从 `templates/knowledge/meta/audit-status.md` 创建（迁移-on-read）
- `mark_audited` — Subagent 完成时引擎写入 `已审计`
- `query_summary` / `query_chapter` / `list_pending` — 供 `AuditStatusQuery` 与 `format_progress_hint`（system Progress 段）

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
| `causality` | 因果图 |
| `evolution_log` | 演变日志 append、末行查询 |
| `index` | `rebuild_index`, `ensure_index` |
| `derive` | 派生快照（纯函数，不自动写盘） |
| `scaffold` / `scaffold_templates` | 新建作品目录树 |
| `project_tree` | IPC 文件树列表 |
| `parser` | YAML frontmatter |

已移除的专用模块：`scene`、`prop`、`faction`、`skill_encyclopedia`、`regex_cache`（逻辑合并为通用 Markdown + 工具层）。

### 1.6 derive — 派生快照

纯函数，不自动写盘：
- `derive_character_snapshot`
- `derive_foreshadow_categories`
- `derive_relation_cross_index`

由 Agent 或 **KnowledgeDerive** 工具调用。

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
