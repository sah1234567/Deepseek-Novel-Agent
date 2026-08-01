# Novel Agent — 完整架构框架

> 项目入口：[README.md](README.md) · Crate 专题：[docs/README.md](docs/README.md) · 提示词：[prompt/](prompt/)

**目录**

1. [架构总览](#1-架构总览) — 分层 / 数据归属 / Crate 依赖 / 设计原则 / 前后端边界
2. [数据流](#2-数据流) — 主流程 / 作品与会话 / Fork 子 Agent / System Prompt / 前端状态与 IPC（含命令事件全表）
3. [技术栈](#3-技术栈)
4. [Crate 文档索引](#4-crate-文档索引)
5. [验收与发布](#5-验收与发布)

---

## 1. 架构总览

### 1.1 系统分层

```
React Frontend (ui/src/)
  ChatPanel（TranscriptView + Sticky 本轮提问 + 压缩 Banner）· StatusBar（Todo 下拉常驻）· SettingsPanel（弹窗）· FileTreePanel · Graph 面板（plan 节点 / Book Loop / HITL）

Tauri IPC（commands → engine_loop 单任务队列；graph_* 与 chat 并行注册）

Rust Backend（12 个 crate，单向依赖）
  Graph-Primary：`novel-graph` 持有 plan-graph + GraphTracker；聊天会话 node-scoped（focus / NodeObjective）。作者 **编排 | 互动** 双模式：编排侧用 Interview/Orchestrator 工具 + orchestrator 提示；互动侧（focus 节点）用 NodeExecution 工具 + node-execution 提示。
```

### 1.2 Agent 根目录与数据归属

```
novel_agent/
├── templates/                        ← 脚手架蓝图（git 跟踪，运行时读盘）
├── skills/                           ← Agent 级 Skill（固定；作品可覆盖）
├── works/{作品名}/                   ← 作品实例（gitignore）
│   ├── AGENTS.md · knowledge/ · chapters/ · memory/
│   ├── knowledge/meta/plan-graph.json · graph-state.json · checkpoints/  ← Graph-Primary 编排
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
| `knowledge/meta/plan-graph.json` | 作品 | 随作品切换；Loop 游标在 `graph-state.json` |
| `api_config.json` | Agent | 不变 |

切换作品时更新 `active_project` 并重建 engine；文件树与会话列表读取当前作品 DB。

### 1.3 Crate 依赖关系

```
novel-server (Tauri IPC)
  ├─ novel-graph (plan schema / GraphTracker / gates / Book Loop；仅依赖 novel-config)
  └─ novel-core (AgentEngine, Fork, Hook, dynamic_context)
       ├─ novel-graph
       ├─ novel-deepseek (ChatClient, SSE, cache, tool_args)
       ├─ novel-tools (+ Graph* / AuditStatusUpdate；→ novel-graph)
       ├─ novel-knowledge (scaffold 静态拷贝 plan JSON；↛ novel-graph)
       ├─ novel-state (SQLite)
       ├─ novel-compaction (4-level)
       ├─ novel-config (paths, settings, api_config.json)
       ├─ novel-skills (agent skills/ only)
       ├─ novel-memory (记忆类型/选择/提取/prefetch；↛ novel-knowledge)
       └─ novel-logging
```


### 1.4 核心设计原则

| 原则 | 说明 |
|------|------|
| **Graph-Primary 编排** | 全书推进由 `plan-graph.json` + `GraphTracker` 决定；模型在**当前节点**内 ReAct，不自主另选全局编排路径 |
| **节点会话** | `focused_node_id` 绑定作者聊天；多 `running_node_ids` 可扇出并行（写路径不相交）；交接用 `NodeHandoff`（summary + files_touched + artifacts） |
| **审计 = InvokeSkill** | 主路径：`audit-plan` / `audit-knowledge` / `audit-craft` + `AuditStatusUpdate` 写台账。**ForkSubAgent 非主审计路径**（可选隔离 helper，仍只读）；审计报告落盘 `knowledge/meta/audits/`（[novel-knowledge.md §1.1.2](docs/crates/novel-knowledge.md)） |
| **Workflow Skill** | `novel-planning` / `chapter-writing` / `revision` / `post-chapter-checklist` 为节点工位手册；顺序由 Graph deps/loop 决定 |
| **自主写作模式** | `skills/autonomous-writing/SKILL.md`（新会话 Unattended 提示 InvokeSkill；中途切换经 `prompt/permission-mode-enter.md` / `-exit.md` 注入）。含自主循环、审计降频（阶段自适应）、暂停条件 |
| **Session 重建压缩** | 超阈值：**先** archive 全量 → `refresh_system_dynamic_sections`（Index/Memory/Progress/Skills 读盘刷新）→ `[上下文刷新]` user → 5 轮 ReAct。摘要模板含「上一章衔接锚点」「活跃伏笔」；3 次失败静默 skip。详见 [novel-compaction](docs/crates/novel-compaction.md) |
| **Session 双轨存储** | `message_archive`（UI 全历史，按 `compaction_epoch`）+ `messages`（API 工作集）；前端 Turn 级懒加载 + 内存预算（驻留 6 / VIEW 6 / 硬顶 18 轮） |
| **中断与 token 估算** | `AbortController` 立即断开 SSE 流；中断后轻量请求（`max_tokens=1`）估算 prompt_tokens |
| **Skill 二级加载** | Agent 级 `skills/` + 可选作品级（同 id 覆盖）；摘要进 system prompt，正文经 InvokeSkill |
| **脚手架仅磁盘** | `templates/` 缺失 → `TemplatesNotFound`；无 embed fallback |
| **Hook opt-in** | `default_hook_config` 默认空；用户 settings 可启用 PostToolUse Hook |
| **确定性校验层** | 只读工具 `ChapterLint`（反 AI 味七项）/ `WorkHealthCheck`（六项健康聚合）/ `ChapterDiff`（REGATE 对比）——机械检查纯函数化，LLM 审计专注语义 |
| **知识契约与闭环** | 知识文件结构契约 + 审计落盘 + findings→规则沉淀（rejected_path/style 记忆），详见 [novel-knowledge.md §1.1.2](docs/crates/novel-knowledge.md) |
| **单队列 Engine** | 所有 IPC 经 `engine_loop` 串行 |
| **流式 Tool 早执行** | arguments JSON 完整即 dispatch；Allow 立即执行、Ask 等 approve、Deny 流末注入 error |
| **读盘经济** | `prompt/shared-base.md` §2 + `read_economy` 硬限：knowledge 类 >80 行拒绝注入、Grep 默认 80 匹配、Read 256KB 上限 |
| **Tool 谓词方法 (OCP)** | 新增 Tool 覆盖 predicate 方法（`is_always_allowed` / `can_write_outside_plan_dir` / `tracks_skill_references` 等）替代硬编码名称匹配 |
| **权限引擎独立** | `check_permissions` 策略在 `permission.rs`，Tool trait 为薄委托 |
| **Turn 续跑预算** | 续跑 inner turn 预算按当前 turn 内已消耗量计算，避免长会话提前触顶 |

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

**Fork 报告注入：** 工具路径完成后注入一条 `[子 Agent 完成: {type}]` 摘要；完整 transcript 写 `fork_messages`，不进入主 LLM prompt。PostToolUse 路径（KnowledgeAuditor hook）**故意不注入**主会话（避免污染上下文）。主 Agent 读取报告末尾 **`## 接下来（主 Agent 必读）`** 自行决策。

**流式 Tool 时序：** arguments JSON 完整即触发权限检查——Allow 立即执行并流中 poll；Ask 写入 pending；Deny 流末注入 error。流结束按 id 去重，流末 ToolCallRequest 幂等。

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

**Session Turn vs ReAct loop：** **Session Turn**（`turn_number`）= 用户一条消息及完整 inner loop；**ReAct loop**（`inner_turn`）= Turn 内单次 LLM→工具循环。一次用户消息可多次 LLM 调用，但 `total_turns` 只 +1。

**磁盘上的 `session_{uuid}/` 文件夹**（`.novel/logs/`）是审计 JSONL，不是 UI 会话列表的数据源。

**IPC 注意：** 前端 `invoke` 参数 **camelCase**（如 `{ sessionId }`），对应 Rust `session_id`。

**Read file cache（内存 + SQLite 子表）：** 每 path 一条 `ReadCacheEntry`（DashMap）+ `session_read_cache` 子表持久化；Resume anchor 校验后 hydrate，否则 transcript 重放 rebuild；Compaction 后 rebuild + reconcile。规则：partial Read/Tail 窗口并集；Edit 行域 ⊆ committed span（R1）；`replace_all` 升整文件；同路径 `file_op_locks` 串行。`AppStatus.sessionCacheHit/Miss` 是 **LLM token 缓存**，与 read file cache 无关。详见 [novel-tools.md §1.4](docs/crates/novel-tools.md)。

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
| PlanAuditor / KnowledgeAuditor / ChapterCraftAnalyzer | **`skills/audit-*/SKILL.md`** 全文（运行时加载）+ 运行时约束 + `---` + 简短 task |
| **GeneralPurpose** | `prompt/agents/general_purpose.md` 短壳 + 运行时约束 + `---` + **## 自定义任务** + 完整 task |

**LLM tools 与缓存：** 子 Agent API 的 `tools` 按 catalog 过滤（`tool_schemas_for_agent`），仅暴露声明工具。System prompt 精简为 shared-base + skill body，不含编排器指令。

审计手册 SSOT 为 **`skills/audit-plan|audit-knowledge|audit-craft`**（节点主路径 InvokeSkill；可选 Fork 隔离时 `format_fork_task` 同文加载）。GeneralPurpose 薄壳仍 `include_str!`。可 fork 类型与 `max_react_loops` 见 **`FORK_AGENT_CATALOG`**。

**Subagent 写入门控：** `subagent_mutator_gate`（`subagent_queue` 未接线时拒绝 Write/Edit/TodoWrite；主会话始终 `subagent_queue: Some`）。

**代码审查清单（禁止替 Agent 决策）：** 无按路径禁止 Write 的引擎逻辑（主 Agent 路径）；无 Write 后默认自动 KnowledgeAuditor 入队（除非用户 opt-in hooks）；无 `fork_handoff` 引擎解析驱动 Fork 链；Subagent 只读由 fork 角色 prompt + 执行层门控保证，LLM `tools` schema 按 catalog 过滤。

**UI 事件（fork 经 scoped Tauri 事件更新 overlay，不 append 主 Chat）：**

| 事件 | 用途 |
|------|------|
| `sub-agent-started` / `sub-agent-complete` | 更新 `forkRuns`；payload 含 `forkRunId`、`parentToolCallId`（有则 tool 路径，无则 hook） |
| `sub-agent-stream` / `sub-agent-tool` | **仅 overlay 已打开且 `subscribe_fork_stream` 后** emit；listener 按 `openForkRunId` 防御 dispatch |
| `assistant-segment-complete` | 含可选 `forkRunId`；主聊天或 overlay 分段 finalize |
| `subscribe_fork_stream` / `unsubscribe_fork_stream` / `get_fork_messages` IPC | overlay 打开/关闭注册（不经 engine 队列）；`get_fork_messages` 先 setOpenForkRunId → subscribe → SQLite hydrate |
| `interruptible-status-changed` | 主会话 `hasInterruptibleToolInProgress` 变更（替代 streaming 期间 500ms 轮询） |

**前端渲染：** tool 路径在 `SegmentGroup` 内渲染 `SubAgentForkCard`（与 Agent 同构，标题 `Subagent · {类型}`）；hook 路径经 `HookForkCards` 列在 `ScrollViewport` 最底部（不在 transcript 时间线）。进入打开 `SubAgentOverlay`（透传 `forkRuns` 与审批回调；overlay 内 approve/deny 仍走**主** engine 队列）。`AppStatus.hook_running` 反映 drain 状态，StatusBar 无单独 sub-agent chip。

### 2.4 System Prompt 与动态上下文

**初始构建：**

| 段 | 来源 |
|----|------|
| 共享底座 | `prompt/shared-base.md`（工具约定、权限、Memory、禁止项） |
| 编排器 | `prompt/orchestrator.md`（图生命周期、节点激活协议、PlanBuilder、Gate 评估） |
| 自主模式 | `skills/autonomous-writing/SKILL.md`（Unattended 会话切换时注入用户消息） |
| AGENTS.md | 作品根 |
| INDEX | `knowledge/INDEX.md`（≤2000 字） |
| Skills | `skills/` 摘要 only（压缩时读盘刷新；正文经 InvokeSkill → `[上下文刷新]`） |
| Memory | `memory/`（≤4KB；含 rejected_paths 禁区记忆，Flash 选择器优先选出） |
| Progress | 章节数 + 下一章 + 大纲计划章数 + **阶段**（开局/中期/收尾）+ 结构单元 + 未完成会话待办 + **审计台账**（PA/KA/CCA 通过至）+ **剧情状态 digest**（最近章节事件 + 人物快照 + 活跃伏笔，≤800 字，空作品不输出）+ **Graph 段**（focused NodeObjective / Loop 摘要 / Ready 节点） |

**Compaction 后（API 工作集）：** system（AGENTS/Workspace 冻结 + Index/Memory/Progress/Skills 摘要 读盘刷新 + 权限检查决定是否注入自主模式指令）→ `[上下文刷新]` user（Skill 全文 + 会话摘要）→ 最近 5 轮 ReAct。压缩摘要含两个新增字段：「上一章衔接锚点」（原文末3句+细纲摘要）和「活跃伏笔」（未来5章待回收伏笔ID），加速压缩后恢复。Memory / INDEX / Progress **仅在 system 对应节**，不在 `[上下文刷新]`。

**UI 全历史：** 每次压缩前整表写入 `message_archive`；前端 archive 区 + `CompactionDivider` + active 工作集。

### 2.5 前端状态与 IPC

**前端模块分层（`ui/src/`）：**

| 路径 | 职责 |
|------|------|
| `ipc/commands.ts` / `ipc/events.ts` | 命令 / 事件名字符串常量（与 `src-tauri`、`event_payload` 对照） |
| `transcript/` | FSM 状态机 + 事件 payload 类型 + Turn 懒加载 / 内存预算纯函数（`machine` / `mapEvents` / `eventPayloads` / `turnMemoryPolicy` / `liveTail` / `loadPolicy` 三档：驻留 6 / VIEW 6 / 硬顶 18） |
| `hooks/useAgentTauriListeners.ts` | Tauri listen 注册（自 `useAgent` 拆出） |
| `hooks/useTranscriptLoader.ts` | layout bootstrap、Turn 懒加载 IPC、`planMemoryReconcile` → `EVICT_TURNS` 编排、贴底防抖收缩 |
| `components/chat/` | ChatPanel、TranscriptView、AskUserQuestionBlock、ChatInputBar、ToolUseCard、SubAgentForkCard |
| `graph/` | Graph 画布（GraphCanvas、useGraphState、useLoopControls、HitlModal） |
| `context/` + `hooks/useAgent.ts` | 全局 Agent 状态 Provider + TranscriptMachine 状态机 |

**AppStatus：** session ID、权限模式、作品名、turn 序号、待办列表、token 统计、`hook_running`（= `drain_in_progress`，仅 API 字段）、`hasInterruptibleToolInProgress`。

**状态刷新：** token 四字段由 `session-tokens-updated` 事件驱动；todos 由 `session-todos-updated` 驱动；其余 `get_app_status` 每 30s 轮询 + `turn-complete` / `permission-mode-changed` refresh。会话切换由 invoke 调用方在成功后 `refresh()`，**不在** `session-resumed` 上 refresh（竞态，见 §2.5.1 脆弱点 3）。

**invoke 参数：** camelCase（`resume_session({ sessionId })`、`answer_question({ toolCallId, answers: { selections, customText } })`）；命令/事件全表见 §2.5.1。特殊 payload：`tool-call-request` 的 `result` 阶段**不含** `toolName`；`ask-user-question` 的 `questions[]` 用 camelCase（`allowMultiple` / `allowCustom`）。

**前端 UX / 懒加载（详见 [docs/README.md §前端 UI 概要](docs/README.md)）：** `ScrollViewport` 近底跟随（128px）+ `pinAndScrollToBottom` + sticky-prompt-header + Turn 折叠；`[上下文刷新]` 渲染为单气泡 `ContextRefreshBubble`（折叠显示审计状态预览 + Skill 名）；压缩后时间轴 `archive → CompactionDivider → 刷新气泡 → active turns`。Turn 级懒加载：驻留 6 / VIEW 6 / 硬顶 18 轮，`planMemoryReconcile` 统一预取与淘汰（`EVICT_TURNS` 仅卸 FSM 正文，DB 保留）；`archives[].retainedMinTurn/MaxTurn` 记录压缩保留范围。`session-resumed` 仅清 streaming/fork，Transcript 重建在 invoke 完成 + `get_app_status` 更新 `sessionId` 后由 `useTranscriptLoader` 执行。

### 2.5.1 IPC 完整契约（命令表 / 事件表 / 链路 / 脆弱点）

**命令全表（36 个）：**（TS 调用点 → novel-server 实现）

| 命令 | 前端调用点 | novel-server 实现 |
|------|-----------|------------------|
| send_message | useAgent.ts | turn.rs |
| interrupt | useAgent.ts | turn.rs（不进 engine_loop） |
| approve_tool / deny_tool / answer_question | useAgent.ts | turn.rs |
| get_app_status | useAppStatus.ts | settings.rs |
| set_permission_mode / set_interaction_mode | useAppStatus.ts | settings.rs |
| init_novel_project | useAppStatus.ts | project.rs |
| create_session / resume_session | useAppStatus.ts | session.rs（engine 替换） |
| create_work / open_work | useAppStatus.ts | project.rs |
| list_works / list_sessions | useAppStatus.ts | project.rs / session.rs |
| list_project_files / read_project_file | useProjectFiles.ts | project.rs |
| update_session_todo | useAppStatus.ts | settings.rs |
| get_api_config / set_api_config | useAppStatus.ts | settings.rs |
| get_fork_messages | useAgent.ts | session.rs |
| subscribe_fork_stream / unsubscribe_fork_stream | useAgent.ts | fork.rs |
| get_session_transcript_layout / _message_turns / _archive_turns | transcript/service.ts | session.rs |
| graph_get_state | useGraphState.ts | commands/graph.rs |
| graph_activate_node / graph_start_node / graph_clear_focus | useGraphState.ts | graph.rs |
| graph_approve / graph_reject / graph_reopen | useGraphState.ts | graph.rs |
| graph_loop_pause / graph_loop_resume / graph_loop_list_history | useLoopControls.ts / useGraphState.ts | graph.rs |

> 2026-08 已移除 6 个死命令（后端实现 + src-tauri wrapper + main.rs 注册 + 前端映射 + 测试断言五层同步删除）：`graph_get_node`、`graph_get_loop`、`graph_loop_set_target`、`graph_loop_set_cursor`、`graph_preview_template`、`graph_apply_template`（git 历史可溯，未在前端调用）。同批清理：`GraphApplyTemplateTool`（agent 工具，已废弃且被排除出全部可见性清单）、`novel-graph` 的 `checkpoint` 模块 / `load_handoff` / `write_default_plan_file` / `set_loop_setting` / `set_loop_cursor` / `GraphNodeRuntime.session_id`。

**事件全表（22 个）：**

> fork 事件（`sub-agent-*` 等）的 payload 细节与订阅机制见 §2.3「UI 事件」表。

| 事件 | emit 源头 | 前端消费 |
|------|----------|---------|
| stream-chunk | event_payload/stream.rs | useAgentTauriListeners → RAF 批量 |
| tool-call-request | event_payload/tool.rs | FSM TOOL |
| turn-complete | stream.rs | reloadActiveTail + refresh |
| ask-user-question | tool.rs | 暂停 FSM + 面板 |
| assistant-segment-complete | stream.rs | 段渲染 |
| session-tokens-updated | stream.rs | StatusBar patch |
| session-todos-updated | stream.rs + settings.rs | todos patch |
| session-resumed | engine_ipc.rs | 清流式状态 + 刷文件树 |
| permission-mode-changed / interaction-mode-changed | engine_ipc.rs | 全量 refresh |
| compaction-progress | event_payload/compaction.rs | Banner + resetAndBootstrap |
| sub-agent-started / sub-agent-stream / sub-agent-tool / sub-agent-complete | event_payload/subagent.rs | Fork overlay |
| interruptible-status-changed | event_payload/status.rs | 中断标志 |
| graph-state-changed | graph_emit.rs + stream.rs | 拉 snapshot |
| graph-plan-committed | tool_apply.rs（turn 后 flush）+ stream.rs | 开面板 |
| graph-hitl / graph-approval-required | graph_emit.rs | HITL modal |
| graph-loop-changed / node-session-reset | graph_emit.rs | Loop 面板 |

**一条消息的完整链路：**

`ChatInputBar` 提交 → `invoke("send_message")` → src-tauri 转发 → `EngineCommand::SendMessage` 入 mpsc → engine_loop 消费者 → `AgentEngine.handle_message_with_events`（turn 循环 + LLM 流式 + `StreamingToolDispatch` → `ToolExecutor` 执行）→ `Event` 流经 event_tx → `StreamCoalescer`（50ms 合并）→ `emit_core_event` → `app.emit` → 前端 `mountTauriListeners` → FSM → 渲染。

**契约脆弱点（改动前必读）：**

1. **双事件通道**：graph 事件有两个源头（IPC mutate 路径 `commands/graph.rs` + 工具路径 `events.rs`→`graph_emit.rs`），`graph-state-changed` payload 形状不一致（全量 snapshot vs `{source:"tool"}`）——前端必须自行 re-invoke `graph_get_state`
2. **手写 `json!`**：event_payload 的 tool.rs/subagent.rs/compaction.rs/stream.rs 手写 payload，新增字段易漏——优先在 DTO struct 加字段
3. **会话切换竞态**：engine 替换（create/resume/switch）与 `session-resumed` 事件存在竞态——前端不在 session-resumed 上 refresh，invoke 完成后 refresh（见 §2.5）
4. **事件转发生命周期**：`spawn_event_forwarder` 每次 send/approve/deny/answer 新 spawn
5. **DTO 双端镜像**：`AppStatus`（engine_loop.rs ↔ useAppStatus.ts）、`GraphStateSnapshot`（graph types）、`UiTurnBundle`（dto.rs ↔ service.ts）——改字段需两端同步 + `ui/src/test/acceptance/*.test.ts` 守护

**新工具接入（无需前端改动）：**

1. `crates/novel-tools/src/novel/<tool>.rs` 实现 `Tool` trait（trait_def.rs）
2. `novel/mod.rs` 加 `mod` + `pub use`
3. `lib.rs default_registry()` 注册（自动进 LLM 工具列表，hooks.rs 由 registry.names() 驱动）
4. `tests_extra.rs` 惯例枚举补名
5. 可选：`ui/src/utils/tools.ts` 加中文显示标签（缺省优雅降级为空串）

### 2.6 用户中断

`AbortController` 贯穿 LLM 流与工具：Esc → 立即断开 SSE 流；发送新消息 → 中断当前流但不添加中断消息。中断后 partial assistant 持久化 + 缺失 tool_result 补填；emit `TurnComplete`（`was_interrupted: true`），**不**经 `Error` 事件。为保持 token 计数准确，中断后独立发一条轻量请求（`max_tokens=1`）估算 prompt_tokens（三分类拆分不精确，仅为估算）。

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

## 5. 验收与发布

开发：`cargo tauri dev`（Vite HMR）。Release：`cargo tauri build`。

- **本地/CI 验收脚本与 GitHub Actions 门禁表**：见 [docs/README.md §CI/CD](docs/README.md)；完整脚本表见 [scripts/README.md](scripts/README.md)
- **打包发布**：须随应用部署 `templates/` 与 `skills/`（与开发态相同布局）
- **依赖安全**：`ci-frontend.sh` 要求 `pnpm audit` 无 critical；`ci-security-audit.sh` 使用 `cargo audit --deny warnings`，Tauri 桌面传递依赖的已知 advisory 列在 `.cargo/audit.toml`（GTK3/unic/glib），升级 Tauri 时需复查
- **清理作品会话库**：`scripts/reset-work-databases.ps1`（或 `.sh`）— 仅删 `works/**/.novel-agent/state.db*`，不影响 `knowledge/`、`chapters/`、`settings.json`

