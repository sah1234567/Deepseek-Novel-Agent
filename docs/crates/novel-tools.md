# novel-tools — Tool 系统

> 所属项目: [Novel Agent](../../README.md)

---

## 1. 业务逻辑

### 1.1 Tool Trait

每个工具实现统一的 `Tool` trait（name、description、input_schema、validate_input → check_permissions → call）。只读工具的 `interrupt_behavior` 为 Cancel（可被 submit-interrupt 中断），写操作（Write/Edit/Bash）为 Block。**参数命名**：schema 的 properties 键须使用 snake_case（DeepSeek API 要求）。

### 1.1.1 路径 API（`paths.rs`，跨平台）

| 函数 | 用途 |
|------|------|
| `extract_file_path` | Read/Write/Edit/Tail 必填 `file_path` |
| `optional_file_path` | 权限、Hook、进度追踪侧链 |
| `optional_search_root` | Grep/Glob 可选 `search_root`（缺省 = 作品根） |
| `normalize_rel_path` | `\` → `/`，比较与 read_economy 分类 |
| `resolve_under_project` | 相对路径 + `project_root` → `PathBuf`；绝对路径直通 |
| `normalize_chapter_progress_path` | 写章进度追踪补 `chapters/` 前缀 |

**Schema 键名：** 文件工具只用 `file_path`；Grep/Glob 搜索根用 `search_root`（不再接受 `path` 别名）。

### 1.2 ToolRegistry

`default_registry()` 注册 **24** 个工具（无参数；`project_root` 由 `ToolContext` 提供）：

**11 个 builtin：** Read, Write, Edit, **Tail**, Grep, Glob, Bash, WebSearch, InvokeSkill, TodoWrite, AskUserQuestion

**13 个 Novel 专属：**
CharacterSearch, PlotGraph, PlotGrid, ForeshadowTracker, Stats, Corkboard, CharacterRotate, **ForkSubAgent**, ImpactAnalysis, KnowledgeDerive, **AuditStatusQuery**, TrackingQuery, RelationQuery

**AuditStatusQuery：** 只读查询 `knowledge/meta/audit-status.md`。`operation`: `summary` | `chapter` | `pending`；`pending` 需 `audit_type`: `pa` | `ka` | `cca` | `any`（snake_case）。

（对话/节奏/情感分析由 **Subagent** ChapterCraftAnalyzer 承担，非独立主会话工具。）

### 1.3 权限系统

**PermissionMode：**

| 模式 | 行为 |
|------|------|
| Normal | 写操作 Ask 用户确认；TodoWrite（`is_always_allowed`）直接 Allow；InvokeSkill 为只读（`is_read_only`） |
| Plan | 只读任意路径；Write/Edit 仅 `plan/`（UI 切换，无 EnterPlanMode 工具） |
| Auto | 写操作 Allow；AskUserQuestion 仍弹窗 |
| Unattended | 写操作 Allow；AskUserQuestion 不弹窗，模型自行决策 |

**PermissionResult：** Allow / Deny / Ask

**ToolContext** 为每个工具调用提供：权限配置（mode/deny_rules/always_allow）、项目路径、会话 DB（TodoWrite 读写）、权限模式覆盖（前端下拉切换）、**内存** Read 文件缓存（同 path 去重、Edit 行域门禁、Edit 后 patch 并标记 `EditPatched` 禁用 dedup；**不持久化**到 SQLite；resume 空表）、同路径 `file_op_locks`（Read/Tail/Edit/Write 串行）、Skills 目录、fork 控制（`allow_fork` / `subagent_queue`）。Compaction 后 `clear_read_file_cache()`，避免 dedup 指向已摘要的历史 tool_result。

**写路径约束：** 仅 `validate_write_root`（作品 sandbox 内 + 非受保护路径）。无 `allow_chapter_write` / 章节专禁。

### 1.4 Read-before-write 与 read cache

Normal 模式下 Write/Edit 要求目标 path 已在 `read_file_cache` 中（本会话曾 Read/Tail）。Plan/Auto/Unattended 可跳过。

**Cache 规则（工具 `call` 内真相源，非 middleware）：**

| 规则 | 行为 |
|------|------|
| R0 窗口并集 | 同 mtime 的 partial Read/Tail 合并 cached span（例 80–100 + 50–90 → 50–100），从磁盘重切 `raw_content`；读盘经济上限仅约束**单次** tool_result，不限制 merge 后 cache 累计行数 |
| R1 Edit 行域 | **单次替换**须首匹配行在 **committed** span 内；`replace_all` 有 session cache 时跳过 R1（盘上字节匹配即可） |
| R2 Edit patch | 单次替换 partial cache 增量 patch；`replace_all` 后 cache **升为整文件**；Write 仍整文件 `WriteRefresh` |
| R3 Compaction | 压缩后清空 cache |

**Pipeline 分层：** `format_tool_result_for_llm` 仅追加 LLM 文案（`enhance_tool_error_for_llm` / `[fact]` / `[read-dedup]`），不读写 cache。

### 1.5 AskUserQuestion

返回 `ToolError::NeedsUserInput { tool_call_id, payload }`，暂停 turn。前端 `answer_question` 提交选项后继续。

### 1.6 StreamingToolExecutor

SSE 流开始前创建，Allow 权限的工具在 arguments JSON 完整时即可入队执行（不必等流结束）。并发工具经 Semaphore 限流（默认 10），串行工具经 Mutex 独占。`peek_completed_results` 供 UI 流中增量 poll，`get_remaining_results` 在流结束后排空并应用断路器（连续 10 次无进展则 abort）。`discard` 用于中断时丢弃未执行工具。

中断信号经 `AbortSignal` 传入：UserCancel 立即 abort 所有工具；SubmitInterrupt 仅 Cancel 类工具。`execute_one_user_approved` 用于 approve_tool 路径（跳过二次权限检查）。

### 1.7 工具摘要

| 工具 | 说明 |
|------|------|
| Read | 行号分页；knowledge/memory/plan/** 无 limit 且 >80 行工具内拒绝；全量 ≤256KB；相同 path+range+mtime 重复 → stub |
| **Tail** | 读文件物理末尾 N 行（默认 80）；续写衔接；写入 partial read cache（source=Tail）；knowledge/memory/plan/** ≤80 / chapters/** ≤200 行硬限 |
| Write / Edit | Write 整文件入 cache（`WriteRefresh`）；单次 Edit 行域 ⊆ committed span；`replace_all` 跳过 R1 且 Edit 后 cache 整文件；同路径 file lock；stale mtime 守卫 |
| Grep | ripgrep 生态；`search_root` 可选（默认作品根）；默认 ≤80 匹配，`head_limit`/`offset` 分页（`head_limit=0` 无限）；20K 字符硬限 |
| Glob | 通配符搜路径（`*`/`**`/`?`；带 `/` 的前缀 pattern；无 `/` 则任意深度；`dir/*` 等价 `dir/**`）；`search_root` 可选；输出统一 `/` |
| Bash | Shell 命令 |
| TodoWrite | SQLite `session_todos`，merge 模式；Normal 模式直接 Allow |
| CharacterSearch | 人物档案 + 演变日志末行；tool_result 输出 ≤80 行（`KNOWLEDGE_MAX_LINES`） |
| PlotGraph | 因果图 BFS |
| WebSearch | 通用网页搜索（DeepSeek `web_search_20250305`），API Key 与主对话相同：`DEEPSEEK_API_KEY` env 优先，否则 `{agent_root}/.novel-agent/api_config.json`（经 `ToolContext.global_api_config_path` → `novel_config::resolve_agent_api_key`）；失败返回 `ToolError` 而非空成功。原始结果缓存 `{project}/.websearch/`（非 `knowledge/` 正典）。支持 research/similar-works/reader-feedback/trope-reference/fact-check/writing-tips/trending/short-drama 等搜索角度 |
| PlotGrid / ForeshadowTracker | 剧情网格 / 伏笔追踪（含可视化） |
| Stats | 字数、完成率、连续天数 |
| Corkboard | 细纲场景卡片 |
| CharacterRotate | 人物出场轮值（"失踪"检测） |
| InvokeSkill | 按需加载 `skills/{id}/SKILL.md` body（文件夹格式），body 可能含 references Markdown 链接 |
| ImpactAnalysis | 删章/改纲影响 JSON |
| KnowledgeDerive | 知识库派生快照、伏笔分类、关系索引、`INDEX.md` 重建 |
| TrackingQuery | 追踪表查询（场景/道具/势力/时间线/战力/功法），支持 current/range/search 三种操作 |
| RelationQuery | 角色关系与称呼查询，支持双向关系、历史演变、目标过滤 |
| **ForkSubAgent** | 主会话委派子 Agent；入队 `subagent_queue`，`drain_subagent_jobs` spawn/join 后 inject **一条**报告摘要；完整 transcript 在 `fork_messages`（与 PostToolUse KnowledgeAuditor hook 并列，触发路径不同） |

### 1.8 ForkSubAgent

**仅主会话可用**（`allow_fork: true`）。只读、**foreground** 工具：tool 返回后，引擎在本轮 inner turn 内 **等待本批 subagent 全部完成** 再 inject 报告并继续。

**input schema：**

| 字段 | 必填 | 说明 |
|------|------|------|
| `agent_type` | 是 | 见 `FORKABLE_AGENT_TYPE_NAMES`（PlanAuditor、KnowledgeAuditor、ChapterCraftAnalyzer、GeneralPurpose） |
| `task` | 是 | 预定义类型：简短任务；**GeneralPurpose：完整自定义 prompt** |
| `description` | 否 | 日志/UI 短标签（默认 `custom subagent`） |

**`agent_type` 枚举（与 `novel_core::FORKABLE_AGENT_TYPE_NAMES` 同步）：**

`PlanAuditor`, `KnowledgeAuditor`, `ChapterCraftAnalyzer`, **`GeneralPurpose`**

**GeneralPurpose 权限：** 精选工具白名单（Read/Write/Edit/Glob/Grep/CharacterSearch/PlotGraph/Tail/Stats/InvokeSkill/ImpactAnalysis/TodoWrite/WebSearch）；无 ForkSubAgent（禁止嵌套 fork），无 Bash。含 Write/Edit 可在 sandbox 内写 chapters；WebSearch 原始缓存 `{project}/.websearch/`。

**与 PostToolUse 的关系：** 用户可在 `settings.json` 启用 PostToolUse matcher，工具执行后自动入队 **KnowledgeAuditor hook**（轻量遗漏扫描，`source=hook`，不 inject 主会话）。写章收尾仍须手动 Fork 完整 KnowledgeAuditor + ChapterCraftAnalyzer。

### 1.9 Tool Result Pipeline

所有 tool result 路径（流式执行、UI poll、approve_tool、fork 子 Agent）经统一入口 `format_tool_result_for_llm(registry, …)` 处理。Pipeline 顺序：error/soft error 增强 → 读盘行数限制 gate（`Tool::max_output_lines` / `output_limit_exceeded_hint`，经 registry 分发）→ middleware 追加（如 Write/Edit 成功后 `[fact]` 标签、Read 去重后 `[read-dedup]` 提示，`Tool::read_dedup_range_label` / `supports_read_dedup_hint`）。输出包含 `content`（写 SQLite / 送 LLM / 推 UI）和 `hook_preview`（PostToolUse 预览用，在 middleware 追加前截取）。阻塞 I/O 经 `spawn_blocking` 执行。

### 1.10 Tool 扩展点（`Tool` trait predicates）

| 方法 | 用途 |
|------|------|
| `extract_read_span` | Read/Tail committed span 跟踪 |
| `read_dedup_range_label` | `[read-dedup]` hint 的 `range=` 段 |
| `supports_read_dedup_hint` | 是否参与 dedup middleware |
| `max_output_lines` | tool_result 行数上限（`None` = 不限） |
| `output_limit_exceeded_hint` | 超限时的 LLM 指引文案 |
| `is_skill_invocation` / `tracks_skill_references` | InvokeSkill / Read 技能引用状态 |
| `errors_abort_siblings` | 流式执行 sibling abort 策略 |
