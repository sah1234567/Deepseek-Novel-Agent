# novel-tools — Tool 系统

> 所属项目: [Novel Agent](../../README.md)

---

## 1. 业务逻辑

### 1.1 Tool Trait

- `name()` / `description()` / `usage_hint()` / `input_schema()`
- `is_read_only()` / `is_concurrency_safe()`
- **`interrupt_behavior()`** — 默认：只读 → `Cancel`，写操作 → `Block`（Write/Edit/Bash 等不可 submit-interrupt）
- `validate_input` → `check_permissions` → `call`
- **参数命名**：`input_schema()` 的 `properties` 键名和 `required` 数组中的字段名**必须使用 snake_case**（DeepSeek API 要求）；`call()` 中 `require_str` 的 key 须与 schema 一致

### 1.2 ToolRegistry

`default_registry(project_root)` 注册 **22** 个工具：

**6 个通用：** Read, Write, Edit, Grep, Glob, Bash

**2 个交互：** TodoWrite, AskUserQuestion

**14 个 Novel 专属：**
CharacterSearch, PlotGraph, ConsistencyCheck, ChapterRead, WebSearch, PlotGrid, ForeshadowTracker, Stats, Corkboard, CharacterRotate, **ForkSubAgent**, InvokeSkill, ImpactAnalysis, KnowledgeDerive

（对话/节奏/情感分析由 **Subagent** DialogueAnalyzer / PacingAnalyzer / EmotionAnalyzer 承担，非独立主会话工具。）

### 1.3 权限系统

**PermissionMode：**

| 模式 | 行为 |
|------|------|
| Normal | 写操作 Ask 用户确认；TodoWrite / InvokeSkill 直接 Allow |
| Plan | 只读任意路径；Write/Edit 仅 `plan/`（UI 切换，无 EnterPlanMode 工具） |
| Auto | 写操作 Allow；AskUserQuestion 仍弹窗 |
| Unattended | 写操作 Allow；AskUserQuestion 不弹窗，模型自行决策 |

**PermissionResult：** Allow / Deny / Ask

**ToolContext 字段：**
- `permission_mode`, `deny_rules`, `always_allow`
- `project_root`, `session_id`
- `db` — TodoWrite 读写 `session_todos`
- `permission_mode_override` — UI 权限模式下拉（`set_permission_mode`）
- `read_file_cache` — `Option<Arc<DashMap<PathBuf, (u64, String)>>>`（缓存 mtime + 内容，Read-before-write）
- `skills_dir: Option<PathBuf>` — Agent skills 目录，用于 InvokeSkill 解析 skill 路径
- `allow_fork` — 主会话为 true；子 Agent 执行中为 false（禁止嵌套 fork）
- `fork_queue` — 主会话 `ForkSubAgent` 入队 `(agentType, task)`

**写路径约束：** 仅 `validate_write_root`（作品 sandbox 内 + 非受保护路径）。无 `allow_chapter_write` / 章节专禁。

### 1.4 Read-before-write

Normal 模式下 Write/Edit 要求目标 path 已在 `read_file_cache` 中（本会话曾 Read）。Plan/Auto/Unattended 可跳过。

### 1.5 AskUserQuestion

返回 `ToolError::NeedsUserInput { tool_call_id, payload }`，暂停 turn。前端 `answer_question` 提交选项后继续。

### 1.6 StreamingToolExecutor

由 `novel-core::StreamingToolDispatch` 在 **SSE 流开始前**创建；Allow 权限的 tool 在 arguments JSON 完整时即可 `add_tool`（不必等流结束）。

**调度特性：**
1. 并发工具：`Semaphore`（默认 max 10，`settings.agent.max_tool_concurrency`）
2. 串行工具：`Mutex` 独占
3. `peek_completed_results()` — 流中快照已完成结果（供 UI poll，**不 drain**）
4. `get_completed_results()` — 取出并清空已完成缓冲（turn 结束时）
5. `get_remaining_results().await` — 流结束后排空队列；连续 10 次迭代无进展 → abort 剩余（断路器）
6. `discard()` — 用户中断 / streaming fallback 时丢弃未执行 tool

**Abort 集成（`abort.rs`）：**
- `AbortSignal` / `AbortWatch` — 与 `novel-core::InterruptReason` 对应
- `InterruptBehavior::Cancel | Block` — 决定 SubmitInterrupt 时是否 synthetic abort
- `get_abort_reason` — UserCancel 立即 abort；SubmitInterrupt 仅 Cancel 工具
- `synthetic_error` — 生成 `REJECT_MESSAGE` 或 sibling 错误文本
- `has_interruptible_tool_in_progress()` — 全部 Executing 工具均为 Cancel 时返回 true
- Bash 并行 sibling 失败 → `SiblingError` 级联 abort

`execute_one_user_approved` 用于 `approve_tool`：跳过二次 `check_permissions`，直接执行用户已批准的工具。

### 1.7 工具摘要

| 工具 | 说明 |
|------|------|
| Read | 行号分页；全量 ≤256KB；description 建议先 Grep 再读所需行 |
| Write / Edit | 写/精确替换；Edit 要求 old_string 唯一 |
| Grep | 正则搜索；结果 ≤20k 字符 / 250 条；定位后用 Read offset/limit 看上下文 |
| Glob | 通配符搜路径 |
| Bash | Shell 命令 |
| TodoWrite | SQLite `session_todos`，merge 模式；Normal 模式直接 Allow |
| CharacterSearch | 人物档案 + 演变日志末行 |
| PlotGraph | 因果图 BFS |
| ConsistencyCheck | 9 维一致性扫描 + fix 建议 |
| ChapterRead | head/tail/range 优先；full 仅写前/通章审计 |
| WebSearch | 通用网页搜索（DeepSeek `web_search_20250305`），结果缓存 knowledge/market/。支持 research/similar-works/reader-feedback/trope-reference/fact-check/writing-tips/trending/short-drama 等搜索角度 |
| PlotGrid / ForeshadowTracker | 剧情网格 / 伏笔追踪（含可视化） |
| Stats | 字数、完成率、连续天数 |
| Corkboard | 细纲场景卡片 |
| CharacterRotate | 人物出场轮值（"失踪"检测） |
| InvokeSkill | 按需加载 `skills/{id}/SKILL.md` body（文件夹格式），body 可能含 references Markdown 链接 |
| ImpactAnalysis | 删章/改纲影响 JSON |
| KnowledgeDerive | 知识库派生快照建议；支持 `compressLogs` 操作（调用 L2 压缩演化日志） |
| **ForkSubAgent** | 主会话委派子 Agent；入队 `fork_queue`，`drain_pending_forks` 同步 join 后 inject **一条**报告摘要；完整 transcript 在 `fork_messages`（与 PostToolUse LogIntegrityChecker subagent 并列，触发路径不同） |

### 1.8 ForkSubAgent

**仅主会话可用**（`allow_fork: true`）。只读、**foreground** 工具：tool 返回后，引擎在本轮 inner turn 内 **等待本批 subagent 全部完成** 再 inject 报告并继续。

**input schema：**

| 字段 | 必填 | 说明 |
|------|------|------|
| `agentType` | 是 | 见 `FORKABLE_AGENT_TYPE_NAMES`（含 LogIntegrityChecker、GeneralPurpose） |
| `task` | 是 | 预定义类型：简短任务；**GeneralPurpose：完整自定义 prompt** |
| `description` | 否 | 日志/UI 短标签（默认 `custom subagent`） |

**agentType 枚举（与 `novel_core::FORKABLE_AGENT_TYPE_NAMES` 同步）：**

`ConsistencyChecker`, `LogIntegrityChecker`, `DialogueAnalyzer`, `PacingAnalyzer`, `EmotionAnalyzer`, **`GeneralPurpose`**

**GeneralPurpose 权限：** 精选 13 工具白名单（Read/Write/Edit/Glob/Grep/CharacterSearch/PlotGraph/ChapterRead/Stats/InvokeSkill/ImpactAnalysis/TodoWrite/ConsistencyCheck）；无 ForkSubAgent（禁止嵌套 fork），无 Bash。含 Write/Edit 可在 sandbox 内写 chapters。

**与 PostToolUse 的关系：** 用户可在 `settings.json` 启用 PostToolUse matcher，工具执行后自动入队 **LogIntegrityChecker subagent**（`source=hook`，不 inject 主会话）。与手动 `ForkSubAgent(LogIntegrityChecker)` 是同一 Agent 类型、不同触发路径。

阻塞 I/O 经 `blocking` 模块在 `spawn_blocking` 执行。
