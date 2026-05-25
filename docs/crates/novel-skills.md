# novel-skills — 流派 Skill 系统

> 所属项目: [Novel Agent](../../README.md)

---

## 1. 业务逻辑

### 1.1 Skill 文件格式

采用 Claude Code 文件夹格式——每个 skill 是一个独立目录，包含 `SKILL.md` 主文件和可选的 `references/` 子文件：

```
skills/<id>/
├── SKILL.md              # 主 skill：YAML frontmatter + Markdown body
└── references/           # 可选：按需加载的子文件
    ├── variant-a.md
    └── variant-b.md
```

**SKILL.md YAML frontmatter 字段：** `name`（必填），`description`（必填，首要触发机制），`when_to_use`（触发条件），`allowed-tools`（工具权限声明）。

### 1.2 渐进式加载（三级）

| 层级 | 内容 | 加载时机 |
|------|------|----------|
| 1 — 摘要 | Skill 的 `name` + `description` | Session 创建时注入 system prompt |
| 2 — 主 body | `SKILL.md` 完整正文 | 模型调用 **InvokeSkill** 工具时加载 |
| 3 — References | `references/*.md` 子文件 | 模型读取 SKILL.md body 中的 Markdown 链接后，用 **Read** 工具按需打开 |

### 1.3 加载 API

| 函数 | 说明 |
|------|------|
| `load_skill(path)` | 解析 SKILL.md 文件路径 |
| `load_skills_dir(dir)` | 扫描目录：识别包含 `SKILL.md` 的子目录为 skill，跳过 `_` 开头目录 |
| `load_skills_merged(project_dir, agent_dir)` | **遗留兼容**：作品 `{work}/skills/` 覆盖 agent skill（同 id） |

**运行时策略（`engine.rs::build_initial_prompt`）：**

`build_dynamic_context` 调用 `load_skills_merged(project_skills_dir, config.skills_dir)`，同时加载 agent 级与作品级 skill 并合并（同 id 时作品级覆盖 agent 级）。

**新作品脚手架不再创建 `skills/` 目录**；Skill 统一维护在 agent 根。System prompt 仅注入 name+description 摘要。

### 1.4 InvokeSkill 按需加载

主 Agent 通过 **InvokeSkill** 工具运行时加载 skill body。路径解析：`skills/{id}/SKILL.md`（唯一格式）。

加载后的 SKILL.md body 中若包含 `references/*.md` 的 Markdown 链接，模型使用标准 **Read** 工具打开对应文件。

`invoked_skill_ids` 持久化到 session `metadata_json`；Compaction refresh 重注入已 Invoke 的全文。

### 1.5 merge_skill_requirements

从多个 skill body 提取 `shared-systems/`、`knowledge/` 路径引用，去重合并——策划 workflow 确定需创建的知识库文件。仅扫描主 SKILL.md body。

### 1.6 Workflow Skill

| Skill ID | 用途 |
|----------|------|
| `novel-planning` | 三级策划 SOP |
| `chapter-writing` | 写前准备、反 AI 味、写后同步知识库 |
| `revision` | ImpactAnalysis 级联改稿 |
| `post-chapter-checklist` | 写后知识库核对与一致性检查建议 |

frontmatter 可选 `skill_kind: workflow`。body 末尾含 **`## 本阶段完成后`** 自然语言后续指引。

### 1.7 内置流派

`novel_agent/skills/` 含 **4 个 Workflow** + **30+ 流派** Skill（xianxia、scifi、quick-trans、double-world 等；精确列表以目录为准）。

含 references 子文件的 skill：apocalypse、infinite、plagiarism、esports、transmigration、sports、supernatural、scifi、palace、romance 等。

### 1.7 多世界目录（Skill 驱动）

引擎 scaffold 只建单世界骨架；多世界由 Skill 指导 Agent 创建 `knowledge/worlds/<世界名>/`。

| Skill | 场景 |
|-------|------|
| `double-world` | 双界往返穿越 |
| `quick-trans` | 快穿、多任务世界 |
| `infinite` | 无限流、副本 |
| `gaming` | 现实 + 游戏第二世界 |
| `transmigration` | 单向穿越；双向往返叠加 `double-world` |

穿越者放 `knowledge/characters/`（共享层），各世界原住民放 `worlds/<名>/characters/`。

---

## 2. Prompt 与自然语言编排

编排约束首选 **Prompt / Skill / Subagent 正文中的自然语言**，而非 Rust 里的 if/拦截/自动调度。

| 组件 | 要求 |
|------|------|
| Skill `description` | 首要触发机制，写清「何时 Invoke」 |
| Workflow Skill body | 步骤 SOP + **`## 本阶段完成后`**（主 Agent 经 InvokeSkill 读到） |
| Subagent prompt | 角色约束 + 工作流程 + **`## 最终输出（必须写进返回正文）`** + **`## 「接下来」写作参考`** |
| Subagent 返回报告 | Checker/Analyzer **必须**在报告末尾输出 **`## 接下来（主 Agent 必读）`**（主 Agent 读不到 prompt 文件） |
| `system.md` | 全局 LLM 自主编排原则 + Skill/Subagent 信息通道区分；不写「引擎将自动…」 |
| 禁止 | prompt 中写「引擎将自动 Fork…」「PermissionDenied 会拦截 chapters…」——改写成「你应当…」 |

**语气：** 对模型用「应」「建议」「下一步」；避免 JSON / 结构化交接块供引擎解析。

**代码仅允许：** sandbox 路径校验（`validate_write_root`）、用户权限模式（Plan/Auto）、禁止嵌套 fork。
