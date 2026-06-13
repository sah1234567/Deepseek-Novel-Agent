# Novel Agent — 完整架构框架

---

## 1. 架构总览

### 1.1 系统分层

```
React Frontend (ui/src/)
  ChatPanel（TranscriptView + Sticky 本轮提问 + 压缩 Banner）· StatusBar（Todo 下拉常驻）· SettingsPanel（弹窗）· FileTreePanel

Tauri IPC（commands → engine_loop 单任务队列）

Rust Backend（9 个业务 crate + novel-server，单向依赖）
```

### 1.2 Agent 根目录与数据归属

```
novel_agent/
├── templates/                        ← 脚手架蓝图（git 跟踪，运行时读盘）
├── skills/                           ← Agent 级 Skill（固定；作品可覆盖）
├── works/{作品名}/                   ← 作品实例（gitignore）
│   ├── AGENTS.md · knowledge/ · chapters/ · memory/
│   ├── skills/                       ← 可选：同 id 覆盖 Agent 级 Skill
│   ├── settings.json                 ← 作品级模型 / Hook / 权限
│   └── .novel-agent/state.db         ← 该作品的 sessions / messages / todos
└── .novel-agent/api_config.json      ← 全局 API Key
```

| 资源 | 作用域 | 切换作品时 |
|------|--------|------------|
| `templates/` | Agent | 不变 |
| `skills/` | Agent | 不变（作品 `works/{名}/skills/` 可覆盖同 id） |
| `works/{名}/` | 作品 | `active_project`、`db_path`、`settings_path` 同步 |
| `api_config.json` | Agent | 不变 |

切换作品时更新 `active_project` 并重建 engine；文件树与会话列表读取当前作品 DB。

### 1.3 Crate 依赖关系

```
novel-server (Tauri IPC)
  └─ novel-core (AgentEngine, Fork, Hook, dynamic_context)
       ├─ novel-deepseek (ChatClient, SSE, cache, tool_args)
       ├─ novel-tools (24 tools, StreamingToolExecutor, permission engine)
       ├─ novel-knowledge (scaffold, index, derive)
       ├─ novel-state (SQLite)
       ├─ novel-compaction (4-level)
       ├─ novel-config (paths, settings, api_config.json)
       ├─ novel-skills (agent skills/ only)
       └─ novel-logging
```

### 1.4 核心设计原则

| 原则 | 说明 |
|------|------|
| **LLM 自主编排** | 流程顺序由模型 InvokeSkill / ForkSubAgent / Tool 自行决定；代码仅 sandbox 安全、Plan 模式、禁止嵌套 fork |
| **Fork 即子 Agent** | 从 system prompt 处 fork；预定义类型嵌入 `prompt/agents/*.md` 全文 + 运行时约束；GeneralPurpose 的 `task` 即自定义 prompt 主体 |
| **检查类 Subagent** | PlanAuditor / KnowledgeAuditor / ChapterCraftAnalyzer / GeneralPurpose **全部只读**（报告写正文；Write/Edit 由 `subagent_mutator_gate` 门控）。写章分两层：细纲后 Fork PlanAuditor，正文后同批 Fork KnowledgeAuditor + ChapterCraftAnalyzer。**引擎不硬编码**，由 prompt + 门控约束；KnowledgeAuditor 另可 PostToolUse Hook opt-in |
| **Workflow Skill** | `novel-planning` / `chapter-writing` / `revision` / `post-chapter-checklist` 经 InvokeSkill 加载 SOP；`## 本阶段完成后` 自然语言后续指引；system.md §3.1 含 ASCII 状态机图定义 skill 间调用链 |
| **自主写作模式** | `prompt/autonomous-writing.md`（规则正文）；`prompt/permission-mode-enter.md` / `permission-mode-exit.md`（中途切换前后缀）。**新会话 / 压缩重建**且 Unattended → 规则写入 system；**中途切换** → 前缀合并进**下一条**用户消息（单条 user，不改 `messages[0]`）。含自主循环、审计降频、暂停条件 |
| **Session 重建压缩** | 超阈值时：**先** archive 全量 → `refresh_system_dynamic_sections`（AGENTS/Workspace 冻结；Index/Memory/Progress/**Skills 摘要** 读盘刷新 + 权限模式重新检查）→ `[上下文刷新]` user（Skill 全文 + 摘要）→ 5 轮 ReAct。压缩摘要模板含「上一章衔接锚点」「活跃伏笔」字段加速恢复。`compaction-progress` → 前端 **CompactionBanner**（已接入）。连续 3 次失败静默 skip（重试 UI 为后续 issue） |
| **Session 双轨存储** | `message_archive`（UI 全历史，按 `compaction_epoch`）+ `messages`（API 工作集）；前端 Turn 级懒加载 + 内存预算：`get_session_transcript_layout` + `get_session_message_turns` / `get_session_archive_turns`（`useTranscriptLoader`；贴底驻留 6 / 浏览 VIEW 6 / 硬顶 18 轮，`planMemoryReconcile` 统一预取与淘汰；贴底欠填向上预取） |
| **中断与 token 估算** | `AbortController` 立即断开 SSE 流，drain 请求估算 prompt_tokens 保持 session 总数准确 |
| **Skill 二级加载** | Agent 级 `skills/` + 可选作品级 `works/{名}/skills/`（同 id 作品覆盖 Agent）；摘要进 system prompt，正文经 InvokeSkill |
| **脚手架仅磁盘** | `templates/` 缺失 → `TemplatesNotFound`；无 embed fallback |
| **压缩 DB 同步** | Compaction **先** `archive_session_messages` **再** `replace_session_messages`（工作集）；`invoked_skill_ids` 与 `read_skill_reference_paths` 存 `metadata_json`；建会话即 persist `(0,0)` system + `system_static_frozen` 快照 |
| **Hook opt-in** | `default_hook_config` 默认空；用户 settings 可启用 PostToolUse Hook |
| **单队列 Engine** | 所有 IPC 经 `engine_loop` 串行 |
| **流式 Tool 早执行** | arguments JSON 完整即 dispatch；Allow 立即入队执行；Ask 等 approve；Deny 流末注入 error |
| **读盘经济** | `prompt/system.md` §2.3 + `novel-tools` pipeline（`read_economy` 硬限）：knowledge/memory/plan/** >80 行拒绝注入；Grep 默认 80 匹配、`head_limit`/`offset` 分页、截断自动标注 pagination 信息；Read 256KB 硬限 |
| **Tool 谓词方法 (OCP)** | 新增 Tool 可覆盖 predicate 方法替代硬编码名称匹配：`blocks_nested_fork`、`is_always_allowed`、`can_write_outside_plan_dir`、`allowed_in_plan_mode`、`tracks_skill_references`、`is_skill_invocation`、`errors_abort_siblings`、`extract_read_span` |
| **权限引擎独立** | `check_permissions` 策略从 Tool trait 提取到 `permission.rs`，trait 默认为薄委托；避免 53 行策略引擎驻留 trait vtable |
| **Turn 续跑预算** | 续跑时 inner turn 预算按**当前 turn 内**已消耗量计算，避免长会话因累计 assistant 消息数提前触及上限 |
| **Segment 分段 UI** | 每次 LLM 响应结束为一个 segment；主聊天与子 Agent overlay 各自 finalize 独立气泡（CoT + 正文），通过 `fork_run_id` 区分归属 |
| **聊天区布局** | 用户 / Agent / Subagent 为全宽 `message` 气泡；`AskUserQuestion` 为全宽卡片；普通工具为全宽 `message-tool` + 内嵌 `ToolUseCard`（虚线框）；`ForkSubAgent` 与 Agent 同构 `SubAgentForkCard`。长文本边界强制换行；当前 turn 锚点 `min-height` 折叠较早内容；上滚后 **Sticky 本轮用户提问** 可点回起点 |
| **Session Todo UI** | `TodoWrite` → `session_todos`；**`session-todos-updated`** 事件即时刷新 StatusBar（turn 进行中即可，不经 `get_app_status`）；按钮常驻；有未完成项时高亮；下拉按 **进行中 / 未进行 / 已完成** 分组（已完成划掉）；全部完成时列表空态 |

### 1.5 前后端边界

| 侧 | 目录 | 职责 |
|----|------|------|
| 前端 | `ui/` | React + Vite + Tauri invoke/listen |
| 桌面壳 | `src-tauri/` | commands 注册、启动时校验 `templates/` |
| 后端 | `crates/` | Agent 引擎，无 Node 依赖 |
| 运行时资产 | `prompt/` · `skills/` · `templates/` | 提示词 / Skill / 脚手架 |

---

## 2. 数据流

### 2.1 主流程：作者消息 → Agent 响应

```
send_message
  → 必要时压缩上下文（CompactionProgress 通知前端）
  → LLM 流式调用 + 流中 Tool 调度
  → drain_subagent_jobs（PostToolUse hook + ForkSubAgent 入队 → 并行 spawn → join → 工具 fork 注入摘要报告）
  → TurnComplete（pending 工具/问答时跳过）
```

**Turn 暂停：** 待批准工具或待回答 AskUserQuestion 时暂停，不发送 TurnComplete；批准/拒绝/回答后继续 inner loop。

**Fork 报告注入：** ForkSubAgent 工具路径完成后向主会话注入一条 `[子 Agent 完成: {type}]` 摘要（含 UI 元数据）；完整 transcript 写入 `fork_messages`，不进入主 LLM prompt。PostToolUse 路径（KnowledgeAuditor hook）**故意不注入**主会话，避免污染上下文，仅经 StatusBar / SubAgentOverlay 查看。

主 Agent 读取报告末尾 **`## 接下来（主 Agent 必读）`** 建议，自行决定后续操作。

**流式 Tool 时序：** SSE 流中 arguments JSON 完整即触发权限检查。Allow 立即执行并流中 poll 结果；Ask 写入 pending 等待用户确认；Deny 在流末注入 error result。流结束时按 id 去重，流末 ToolCallRequest 幂等。

### 2.2 作品与会话

**作品切换：** 下拉选择 → 更新 `active_project` + 新建 session。**会话切换：** 列表按最近 LLM 活跃时间降序；选择历史会话恢复，`+` 新建空白会话。SettingsPanel 共用同一会话列表 IPC。

**持久化：** 每作品 `{work}/.novel-agent/state.db`；`list_sessions` 按 `project_root` 精确匹配过滤。

**会话指标（勿混淆）：**

| 字段 / UI | 含义 | 何时更新 |
|-----------|------|----------|
| `total_turns` / 「对话 N 轮」 | **用户对话轮数**（每条用户消息 +1） | `sync_user_turn_count`（发消息时；resume 仅对齐 DB，**不**改时间戳） |
| `api_call_count` | **LLM API 调用次数**（inner loop、子 Agent 计费等） | `accumulate_session_tokens` |
| `cache_hit/miss/completion_tokens` | 会话级三类 token **独立累加** | `accumulate_session_tokens`（主 Agent 与 SubAgent 均参与） |
| `context_tokens` / StatusBar「当前上下文」 | 主 Agent **最近一次主 inner_turn API** 的 `hit+miss+comp`（覆盖写入；SubAgent 与压缩摘要 LLM 计费不更新此字段） | `accumulate_session_tokens(..., update_context_snapshot: true)`；压缩后由下轮主 API 刷新 |
| `last_active_at` / 相对时间 | **最后一次 LLM 请求结束或流式中断** | `accumulate_session_tokens` 或 `touch_last_active_at`；**resume / 纯点击切换不更新** |
| `turnNumber`（AppStatus） | 当前 engine 内用户 turn（≈ `total_turns`） | 内存态，发消息时 +1 |
| `messages.turn_number` | 同一次用户消息触发的 assistant/tool 共享同一 turn | 持久化在 messages 表 |

**Session Turn vs ReAct loop：** **Session Turn**（`turn_number`）= 用户一条消息及其触发的完整 inner loop；**ReAct loop**（`inner_turn`）= Turn 内单次 LLM→工具循环。一次用户消息可产生多次 LLM 调用（`api_call_count` 递增），但 `total_turns` 只 +1。

**磁盘上的 `session_{uuid}/` 文件夹**（`.novel/logs/`）是审计 JSONL，不是 UI 会话列表的数据源。

**IPC 注意：** Tauri v2 前端 `invoke` 参数用 **camelCase**（如 `{ sessionId }`），对应 Rust `session_id`。

**Read file cache（内存 + SQLite 子表）：** 每 path 一条 `ReadCacheEntry`（`EngineShared.read_file_cache` DashMap）；`session_read_cache` 子表持久化当前态（schema v3）；Resume 优先 anchor 校验后 hydrate，否则从 transcript 重放切片 rebuild；Compaction 后 rebuild + reconcile（非裸清空）。工具 `call` 为真相源；API 批末 `flush_dirty_read_cache_paths` 部分 UPSERT。规则：partial Read/Tail **窗口并集**（同 mtime）；单次 Edit 行域 ⊆ committed span；`replace_all` 有 cache 时跳过 R1 且 Edit 后 cache 升为整文件；Write 仍整文件 `WriteRefresh`；同路径 `file_op_locks` 串行 Read/Tail/Edit/Write。`AppStatus.sessionCacheHit/Miss` 为 **LLM token 缓存**，与 read file cache 无关。详见 [`docs/crates/novel-tools.md`](docs/crates/novel-tools.md) §1.4。

### 2.3 Fork 子 Agent

**引擎队列（`EngineShared`）：** `subagent_queue`（`SubagentWorkQueue` / `PendingSubagentWork`）统一承接 ForkSubAgent 与 PostToolUse hook；`drain_subagent_jobs` 并行 spawn `run_subagent_job` 后 join。`session_llm`（`SessionLlmSnapshot`）保存主 Agent 最近一次 API 的 model/thinking，供 drain 与子 Agent 复用。`drain_in_progress` 在任意 subagent drain 期间为 true（不限 PostToolUse）；`AppStatus.hook_running` 与之同义（保留字段名供 Tauri 前端）。

**触发方式：**

| 路径 | 说明 |
|------|------|
| `ForkSubAgent` 工具 | 主会话 LLM 调用；入队 `subagent_queue`，本 turn 内 `drain_subagent_jobs` 等待本批全部完成后注入一条摘要报告 |
| PostToolUse 自动触发 | settings 启用 matcher 后，入队同队列并 drain；KnowledgeAuditor 轻量扫描，**不注入**主会话 |

**LLM 上下文隔离：** 子 Agent 内 assistant/tool 消息仅存 `fork_messages` + 前端 overlay；主 LLM 仅见工具路径的一条报告摘要。Compaction 不合并 `fork_messages`。Hook 路径不注入主会话。Subagent `max_react_loops` 耗尽时注入提醒 + report-only 收尾轮，非硬截断。

**双轨 prompt：**

| agentType | task_message 组成 |
|-----------|-------------------|
| 预定义（Checker/Analyzer 等） | `prompt/agents/{name}.md` 全文 + 运行时约束 + `---` + 简短 task |
| **GeneralPurpose** | `prompt/agents/general_purpose.md` 短壳 + 运行时约束 + `---` + **## 自定义任务** + 完整 task |

**LLM tools 与缓存：** 子 Agent API 的 `tools` 与主 Agent 同源（`main_tool_schemas` / `registry.names()`），与 `messages[0]` system 一并保持 DeepSeek 前缀缓存。`task_message` 中的「建议优先工具」仅作 prompt 指引，不缩减 API schema。

Agent prompt 文件位于 `prompt/agents/*.md`，编译期嵌入（`agent/catalog.rs::system_prompt`）。可 fork 类型与 `max_react_loops` 见 **`FORK_AGENT_CATALOG`**（`crates/novel-core/src/agent/catalog.rs`）。

**Subagent 写入门控：** `subagent_mutator_gate`（`subagent_queue` 未接线时拒绝 Write/Edit/TodoWrite；主会话始终 `subagent_queue: Some`）。

**代码审查清单（禁止替 Agent 决策）：**

- 无 `allow_chapter_write` / `validate_chapter_write` / 按路径禁止 Write（主 Agent 路径）
- 无 Write 后默认自动 KnowledgeAuditor 入队（除非用户 opt-in hooks）
- 无 `fork_handoff` / `## 引擎交接` 引擎解析驱动 Fork 链
- Subagent 只读：fork 角色 prompt + **执行层门控**；**禁止**为只读而缩减 fork LLM `tools` schema

**UI 事件：** fork 实例经 scoped Tauri 事件更新前端 overlay（不 append 主 Chat `messages`）：

| 事件 | 用途 |
|------|------|
| `sub-agent-started` / `sub-agent-complete` | 更新 `forkRuns`；payload 含 `forkRunId`、`parentToolCallId`（有则 tool 路径，无则 hook） |
| `sub-agent-stream` / `sub-agent-tool` | **仅 overlay 已打开且 `subscribe_fork_stream` 后** 由引擎 emit；listener 再按 `openForkRunId` 防御 dispatch |
| `assistant-segment-complete` | 含可选 `forkRunId`；主聊天或 overlay 分段 finalize |
| `subscribe_fork_stream` / `unsubscribe_fork_stream` IPC | overlay 打开/关闭时注册（不经 engine 队列）；session 切换时清空 |
| `get_fork_messages` IPC | `openForkOverlay`：**先** `setOpenForkRunId` → subscribe → 从 SQLite hydrate（含 running） |
| `interruptible-status-changed` | 主会话 `hasInterruptibleToolInProgress` 变更（替代 streaming 期间 500ms `get_app_status` 轮询） |

**前端：** **tool 路径** — `ForkSubAgent` 在 `SegmentGroup` 内渲染 `SubAgentForkCard`（与 `AgentBubble` 同构：`message-assistant`，标题 `Subagent · {类型}`）。**hook 路径** — PostToolUse 触发的子 Agent 经 `HookForkCards` 列在 `ScrollViewport` **最底部**（不在 transcript 时间线内）。**进入** 打开 `SubAgentOverlay`（`TranscriptView mode=fork`，透传 `forkRuns` 与工具审批回调；overlay 内 approve/deny 仍走**主** engine 队列，非 fork 作用域）。返回内容在卡内 `details` 展开。`AppStatus.hook_running` 反映 drain 状态，**StatusBar 无单独 sub-agent chip**。Todo 在 StatusBar 下拉，非独立 TodoPanel。

### 2.4 System Prompt 与动态上下文

**初始构建：**

| 段 | 来源 |
|----|------|
| 静态层 | `prompt/system.md`（含读盘经济 §2.3、写后 Fork 规范、Skill 状态机） |
| 自主模式 | `prompt/autonomous-writing.md`（Unattended 权限时追加注入，含自主循环/审计降频/暂停条件） |
| AGENTS.md | 作品根 |
| INDEX | `knowledge/INDEX.md`（≤2000 字） |
| Skills | `skills/` 摘要 only（压缩时读盘刷新；正文经 InvokeSkill → `[上下文刷新]`） |
| Memory | `memory/`（≤4KB） |
| Progress | 章节数 + TodoWrite |

**Compaction 后（API 工作集）：** system（AGENTS/Workspace 冻结 + Index/Memory/Progress/Skills 摘要 读盘刷新 + 权限检查决定是否注入自主模式指令）→ `[上下文刷新]` user（Skill 全文 + 会话摘要）→ 最近 5 轮 ReAct。压缩摘要含两个新增字段：「上一章衔接锚点」（原文末3句+细纲摘要）和「活跃伏笔」（未来5章待回收伏笔ID），加速压缩后恢复。Memory / INDEX / Progress **仅在 system 对应节**，不在 `[上下文刷新]`。

**UI 全历史：** 每次压缩前整表写入 `message_archive`；前端 archive 区 + `CompactionDivider` + active 工作集。

### 2.5 前端状态与 IPC

**前端模块分层（`ui/src/`）：**

| 路径 | 职责 |
|------|------|
| `types/messages.ts` | `UIMessage` / `ToolCall` / `ForkRunState` 等共享类型（组件与 transcript 直接引用） |
| `ipc/commands.ts` / `ipc/events.ts` | Tauri command / event 名字符串常量（与 `src-tauri/commands.rs`、`event_payload` 对照） |
| `transcript/eventPayloads.ts` | 事件 payload 类型（canonical，供 `mapEvents` / hooks 共用） |
| `hooks/useAgentTauriListeners.ts` | Tauri listen 注册（自 `useAgent` 拆出） |
| `hooks/useTranscriptLoader.ts` | layout bootstrap、Turn 懒加载 IPC、`planMemoryReconcile` → `EVICT_TURNS` 编排、贴底防抖收缩 |
| `hooks/useSlotVisibility.ts` | 已加载 turn slot 可见性 `Set` + timeline envelope（供 `planMemoryWindow` focal） |
| `hooks/useViewportContentFill.ts` | 贴底欠填 DOM 测量（`contentUnderflowRef`，loader 只读） |
| `transcript/loadPolicy.ts` | Turn 内存三档常量（`TAIL_LOADED_TURNS` 6、`VIEW_LOADED_TURNS` 6、`MAX_LOADED_TURNS` 18、`TAIL_CONTENT_UNDERFLOW_PX` 48、`BOTTOM_ANCHOR_THRESHOLD_PX` 128、`TAIL_COMPACT_DEBOUNCE_MS` 400） |
| `transcript/turnLoadPlan.ts` | 相邻 idle 窗口预取与分段 IPC 计划 |
| `transcript/turnMemoryPolicy.ts` | 纯函数：`planMemoryReconcile` / `planMemoryWindow` / `planTailContentFill`、`isInBottomAnchorZone` |
| `transcript/liveTail.ts` | 流式尾轮 orphan turn 选取、`MERGE_TURNS` / `BEGIN_TURN` reconcile |
| `components/chat/AskUserQuestionBlock.tsx` / `ChatInputBar.tsx` | 自 `ChatPanel` 拆出的问答与输入栏 |

**AppStatus：** 包含当前 session ID、权限模式、作品名、turn 序号、待办列表（`todos: { id, content, status }`）、token 统计（三分类累计 + 当前上下文）、项目初始化状态、`hook_running`（= `drain_in_progress`，**仅 API 字段，StatusBar 未展示**）、`hasInterruptibleToolInProgress` 等。

**状态刷新：** StatusBar **token 四字段**由 **`session-tokens-updated` 事件驱动**（主 Agent 与 SubAgent 每次 LLM `accumulate` 成功后推送；SubAgent 不覆盖 `context_tokens`）。**todos** 由 **`session-todos-updated` 事件驱动**（`TodoWrite` / `update_session_todo` 后推送全量列表）。`useAppStatus` 每 **30s** 轮询 `get_app_status` 作非 token / 非 todo 兜底；并在 `turn-complete`、`permission-mode-changed` 时全量 refresh turn 标志等。`hasInterruptibleToolInProgress` 由 **`interruptible-status-changed` 事件**驱动（`useAgent` listen），streaming 期间**不**轮询 `get_app_status`。会话切换由 `resumeSession` / `createSession` / `openWork` 等 invoke 调用方在成功后 `refresh()`，**不在** `session-resumed` 上 refresh（该事件早于 invoke 完成，会与 `sessionBusy` / transcript bootstrap 竞态）。

| 领域 | 主要 Command |
|------|----------------|
| 作品 | `list_works`, `create_work`, `open_work` |
| 会话 | `create_session`, `resume_session`, `list_sessions`, `get_session_transcript_layout`, `get_session_message_turns`, `get_session_archive_turns` |
| 聊天 | `send_message`, `interrupt`, `approve_tool`, `deny_tool`, `answer_question` |
| 待办 | `update_session_todo`（StatusBar 点击循环 `pending` ↔ `in_progress` ↔ `completed`；直写 DB 并 emit `session-todos-updated`） |
| 文件 | `list_project_files`, `read_project_file`（当前 active 作品） |
| 配置 | `get_api_config`, `set_api_config`（全局 json） |
| 脚手架 | `init_novel_project` |
| Fork 回放 | `get_fork_messages` |

**前端 invoke 参数：** camelCase（例：`resume_session({ sessionId })`、`get_session_message_turns({ sessionId: null, fromTurn, toTurn })`、`update_session_todo({ sessionId, todoId, status })`、`answer_question({ toolCallId, answers: { selections, customText } })`）。

**Tauri 事件 → 前端：**

| 事件 | 消费方 | 说明 |
|------|--------|------|
| `stream-chunk` / `tool-call-request` / `assistant-segment-complete` | `useAgent` → Transcript FSM | `tool-call-request` 的 `result` 阶段**不含** `toolName` |
| `ask-user-question` | `useAgent` | payload 中 `questions[]` 使用 **camelCase**（`allowMultiple` / `allowCustom`），由 `event_payload::ask_questions_for_ui` 转换 |
| `turn-complete` | `useAgent` + `useProjectFiles` + `useTranscriptLoader` | 含 turn 级 token 字段（终局 payload 标记）；`phase: start` / `error` 分支；turn 结束 `onTurnComplete` → `useAppStatus` 单次 `get_app_status`（非 token 主路径）；`reloadActiveTail` 同步尾轮 |
| `session-resumed` | `useAgent` + `useProjectFiles` | `useAgent` 清 streaming / fork；transcript 由 `status.sessionId` 变化驱动 `useTranscriptLoader.resetAndBootstrap` |
| `session-tokens-updated` | `useAppStatus` | 每次 LLM 调用后推送；局部 patch `sessionCacheHit` / `sessionCacheMiss` / `sessionCompletion` / `contextTokens` |
| `session-todos-updated` | `useAppStatus` | `TodoWrite` / `update_session_todo` 后推送全量 `todos`；局部 patch，不调用 `get_app_status` |
| `permission-mode-changed` | `useAppStatus` | 全量 refresh |
| `compaction-progress` | `useCompactionProgress` + `ipc/compactionDone` | Banner + `done` 时 `useTranscriptLoader` re-bootstrap（单次总线，避免双 listen） |

**聊天区 UX（纯前端）：** `ScrollViewport` 贴底滚动（近底区 `BOTTOM_ANCHOR_THRESHOLD_PX` 128px 内新气泡自动置底，`followBottomIfAnchored` 双 rAF 跟底；`pinAndScrollToBottom` 发送后强制贴底；`onBottomAnchorChange` 供尾部收缩防抖）；最后一轮 `transcript-turn-anchor` 仅在 **idle** 时占满视口高度（Turn 折叠）；流式 / turn 进行中不垫 `minHeight`；用户气泡紧挨上一轮、assistant/tool 在其下方生长；发送后 `pinAndScrollToBottom` + 近底跟随置底；时间轴仅渲染 loaded 窗口及邻接 idle 占位（避免 4rem×N 虚高）；用户气泡滚出视口上方时显示 **sticky-prompt-header**（点击平滑滚回本轮提问）。`AskUserQuestion` 全宽卡片插在 `pauseAfterSegmentId` 对应段 tools 之后。`[上下文刷新]` 渲染为 **单气泡** `ContextRefreshBubble`：默认折叠显示摘要预览（优先一行「审计状态」）与已激活 Skill 名；展开展示「审计状态」块（若有）+ 会话历史摘要（**非**单气泡合并 system / 5 轮 ReAct；system 不进 UI；Skill 正文不进 UI）。压缩后时间轴：`archive` → `CompactionDivider` → turn 0 刷新气泡 → 保留的 active turns。

**`session-resumed` 事件：** `create_session` / `resume_session` / 切换作品成功后触发；`useAgent` 清 streaming / fork 状态。Transcript 重建在 invoke 完成且 `get_app_status` 更新 `sessionId` 后，由 `useTranscriptLoader` 拉 layout 并加载 turn 0（若有）+ active 尾部 K 轮；`compaction-progress` done 时同样 `resetAndBootstrap`。

**Turn 级懒加载与内存：** 预算单位为 **turn**（一轮 user + 助手/tool 链），非单气泡数。`TranscriptView` 按 `turnSlots` 时间轴渲染 idle 占位；`IntersectionObserver` 触发 `onLoadTurn`；`turnLoadPlan` 在相邻 idle 窗口内预取（跨 compact 不截断），再按 `(kind, epoch)` 分段并行 IPC。bootstrap 与贴底稳定后驻留最近 **6** 轮（`TAIL_LOADED_TURNS`）；上滑浏览以可见 focal 为锚的 **6** 轮 VIEW 窗口（`VIEW_LOADED_TURNS`），超 **18** 轮 loaded 时溢出淘汰（`planMemoryWindow` 优先卸不可见且离 focal 最远；`active maxTurn` 永不淘汰）。贴底欠填（`useViewportContentFill` + `TAIL_CONTENT_UNDERFLOW_PX`）时 `planTailContentFill` 向上预取、禁止 evict。贴底区稳定 `TAIL_COMPACT_DEBOUNCE_MS` 后 `planMemoryReconcile` → `EVICT_TURNS`（流式 / turn 进行中 `compactionPaused` 暂停）。`useSlotVisibility` + `LoadedTurnBlock` 上报可见性与 envelope。`EVICT_TURNS` 仅卸 FSM 正文，DB 全量保留。流式尾轮 orphan 由 `liveTail` 渲染并 reconcile。`CompactionDivider` 为 UI 标记；`get_session_transcript_layout` 的 `archives[].retainedMinTurn`/`retainedMaxTurn` 记录该次压缩前保留进工作集的 turn 范围（`compaction-progress` `done` 事件同步携带）。

### 2.5.1 清理作品会话库

清空全部作品的对话历史：`scripts/reset-work-databases.ps1`（或 `.sh`）。仅删 `works/**/.novel-agent/state.db*`，不影响 `knowledge/`、`chapters/`、`settings.json`。

### 2.6 用户中断

`AbortController` 贯穿 LLM 流与工具：Esc → 立即断开 SSE 流；发送新消息 → 中断当前流但不添加中断消息。

中断后：partial assistant 持久化 + 缺失 tool_result 补填；emit `TurnComplete`（`was_interrupted: true`），**不**经 `Error` 事件弹错误横幅。为保持 session token 计数准确，中断后独立发送一条轻量请求（`max_tokens=1`）利用 prefix cache 获取 prompt_tokens 估算值——注意此请求的三分类拆分不等于原始请求，仅为估算。

---

## 3. 技术栈

| 层 | 技术 |
|----|------|
| 前端 | React 18 + TypeScript + Vite 8 |
| 桌面 | Tauri v2 |
| 异步 | tokio |
| LLM | reqwest SSE → DeepSeek OpenAI 兼容 API |
| 数据库 | SQLite (rusqlite + r2d2), WAL |
| 并发读缓存 | dashmap |
| 图 | petgraph（因果链） |

---

## 4. Crate 文档索引

见 [docs/README.md](docs/README.md)。

---

## 5. 验收脚本

```powershell
# Windows 本地全量（frontend + rust + audit；GHA rust-windows 仅 frontend + rust，audit 在 Ubuntu job）
.\scripts\ci-windows.ps1

# 跨平台本地 CI
.\scripts\ci-local.ps1
```

```bash
# 仅 Rust + Tauri / 仅 nextest（Git Bash）
bash scripts/ci-rust-gate.sh
bash scripts/ci-rust-test.sh
```

见 [scripts/README.md](scripts/README.md)。

开发：`cargo tauri dev`（Vite HMR）。Release：`cargo tauri build`。

打包发布时须随应用部署 `templates/` 与 `skills/`（与开发态相同布局）。

**依赖安全：** `ci-frontend.sh` 要求 `npm audit` 无 critical；`ci-security-audit.sh` 使用 `cargo audit --deny warnings`，Tauri 桌面传递依赖的已知 advisory 列在 `.cargo/audit.toml`（GTK3/unic/glib），升级 Tauri 时需复查。
