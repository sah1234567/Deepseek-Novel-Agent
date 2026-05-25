# novel-core — Agent 循环与 Fork 子 Agent

> 所属项目: [Novel Agent](../../README.md)

---

## 1. 业务逻辑

### 1.1 AgentEngine — 主引擎

`AgentEngine` 管理完整的小说创作会话。

**创建流程：**
1. 加载 `settings.json`（`novel-config`）
2. 创建 SQLite Session
3. 构建 `ToolRegistry`（**22** 个工具，`default_registry`）
4. 创建 `ContextManager`（上下文窗口 + 压缩阈值）
5. `SystemPromptBuilder::build()` 拼装 system prompt（静态层 + 5 个动态段）
6. 消息列表首条为 system message（session 内冻结）

**会话恢复：**
- `AgentEngine::resume(config, session_id)` 从 SQLite 恢复历史
- turn_number 从历史消息推导

**主循环：**
- `handle_message_with_events(content, event_tx)` → `run_inner_turn_loop`
- 每轮：`call_llm_and_execute`（LLM 流式 + **流中 Tool 调度**）→ `drain_pending_hooks` → 下一轮或结束

**公共 API：**

| 方法 | 说明 |
|------|------|
| `new` / `resume` | 新建或恢复会话 |
| `handle_message` / `handle_message_with_events` | 用户消息 |
| `fork(agent_type, task)` | Fork 子 Agent |
| `approve_tool` / `deny_tool` | 批准/拒绝待确认工具（可带 `event_tx` 续跑） |
| `answer_question(tool_call_id, answers, event_tx)` | AskUserQuestion 回答后续跑 |
| `status_snapshot()` | 返回 `EngineStatus` 供 Tauri `get_app_status` |

**EngineStatus 字段：** session_id, permission_mode, hook_running, pending_user_question, turn_number, project_initialized, has_interruptible_tool_in_progress

### 1.2 Fork 子 Agent

**硬约束：** 子 Agent 只能从 `messages[0]`（system prompt）fork，禁止嵌套 fork。

**运行时守卫：** `sub_agent_count` / `sub_agent_running` 在子 Agent 执行期间 > 0，此时 `fork()` 与 `ForkSubAgent` 返回 `NestedForkProhibited`。

**触发路径：**

| 路径 | 执行函数 | 同步/异步 | 主会话 inject | Transcript |
|------|----------|-----------|---------------|------------|
| `ForkSubAgent` 工具 | `drain_pending_forks` → 并行 `run_subagent_async` → join → `inject_sub_agent_report` | **同步（foreground）** | 一条摘要 | `fork_messages` + UI overlay |
| PostToolUse 自动触发 | `drain_pending_hooks` → `run_log_integrity_checker`（LogIntegrityChecker subagent，`source=hook`） | 同步 | **无** | `fork_messages` + UI overlay |
| `Op::ForkSubAgent` / Tauri IPC | `tokio::spawn(run_subagent_async)` | 异步（debug） | 无 | `fork_messages` |

`ForkSubAgent` 工具路径：`drain_pending_forks` 在本轮 tool 执行结束后 **await 本批全部 subagent**，按 fork_queue FIFO 注入 `[子 Agent 完成: …]` 报告（`persist_message_alloc_ex` + `fork_run_id` 元数据，不占 seq 0）。`subagent_result_rx/tx` 为 legacy channel，**无 consumer inject** 主会话。

**双轨 task_message（`prompt_loader::format_fork_task`）：**

预定义类型：
```
{prompt/agents/*.md 全文}

## 子 Agent 运行时约束
…

---

{user_task}
```

**GeneralPurpose（自定义，对齐 Cursor generalPurpose）：**
```
{prompt/agents/general_purpose.md 短壳}

## 子 Agent 运行时约束
…

---

## 自定义任务

{user_task}   ← 主 Agent 编写的完整 prompt
```

父 system prompt 不变 → DeepSeek prefix cache 命中 `[m0]`。

**子 Agent 消息数组：** `[system_prompt, task_message]`（仅 2 条）

**类型注册：**

- `AgentType` 枚举 + `AgentType::definition()` 工具白名单 / max_turns
- `FORKABLE_AGENT_TYPE_NAMES` / `all_forkable_names()` — ForkSubAgent 可用类型（含 LogIntegrityChecker、GeneralPurpose）
- `AgentType::parse(name)` — PascalCase / kebab-case 统一解析（Tauri `parse_agent_type` 亦调用此函数）

### 1.3 Agent 类型与工具

详细 prompt 来自 `prompt/agents/*.md`（`include_str!`）；fork 路径以 `prompt_loader` 为准。**主 Agent 读不到这些文件**——Checker/Analyzer 须在返回报告末尾输出 **`## 接下来（主 Agent 必读）`**；prompt 内 `## 「接下来」写作参考` 仅为子 Agent 写作模板。

| Agent | Max Turns | 工具概要 |
|-------|-----------|----------|
| LogIntegrityChecker | 15 | Read/Grep/CharacterSearch（只读） |
| ConsistencyChecker | 50 | Read/Grep/CharacterSearch/PlotGraph/ChapterRead + ConsistencyCheck（只读，无 Edit/Write） |
| DialogueAnalyzer | 10 | Read/Grep/CharacterSearch |
| PacingAnalyzer | 10 | Read/Grep/Stats/ChapterRead |
| EmotionAnalyzer | 10 | Read/Grep/CharacterSearch |
| **GeneralPurpose** | 20 | 精选 13 工具白名单（Read/Write/Edit/Glob/Grep/CharacterSearch/PlotGraph/ChapterRead/Stats/InvokeSkill/ImpactAnalysis/TodoWrite/ConsistencyCheck）；无 ForkSubAgent，无 Bash |

**Workflow（策划/写章/改稿/写后）：** 经 **InvokeSkill** 加载 `skills/{novel-planning,chapter-writing,revision,post-chapter-checklist}/SKILL.md`，不再使用写稿 Subagent。

**GeneralPurpose：** `task` = 完整执行指令；13 工具白名单（含 Write/Edit chapters），sandbox 路径校验。

### 1.4 System Prompt 与动态上下文

**静态层：** `prompt/system.md` 经 `include_str!()` 编译期嵌入。`SystemPromptBuilder::build()` 将静态层与动态上下文拼接。

**动态层 `DynamicContext`：** agents_md、knowledge_index（≤2000 字符）、memory（≤4KB，截断时追加 WARNING）、progress（章节进度 + TodoWrite）、skill_summaries（仅 name+description 摘要，不含 body）。

**skill 加载：** Agent 级 `skills/` + 可选作品级 `works/{名}/skills/`（同 id 覆盖）；system prompt 只含摘要；body 经 InvokeSkill；references 经 Read 渐进加载。

**读盘经济（prompt）：** `system.md` §2.3 要求 Grep/CharacterSearch 定位 → Read offset/limit 或 ChapterRead head/tail/range → 非必要不全文 Read；Read 全量上限 256KB，Grep 结果上限 20k 字符（工具层 enforce）。

**压缩后序列：** 刷新 system → `[激活 Skill]`（去重 id，有则必选）→ `[会话历史摘要]` → 最近 5 轮 ReAct。

### 1.5 断路器

**ConsistencyCheck 重试保护：** `consistency_fail_count` 计数器，连续返回 `ok: false` 超过 5 次时中断并报错。

**工具停滞保护：** `get_remaining_results` 连续 10 次迭代无进展 → abort 所有剩余工具 + 注入 `ToolError::Internal("stalled")`。

### 1.6 LogIntegrityChecker（opt-in）

`default_hook_config()` 返回**空** `post_tool_use`。用户可在 `settings.json` 启用 PostToolUse matcher 后，`drain_pending_hooks` 同步 fork LogIntegrityChecker（Unattended 模式）。

**写后流程（prompt 强制，非引擎硬编码）：** `system.md` 要求每章 Write + 知识库 append 后**同批 Fork 5 项** Subagent（LogIntegrityChecker + ConsistencyChecker + 三 Analyzer），按报告 Edit 后再向作者宣告完成。PostToolUse Hook 不能替代手动签收。

### 1.7 AskUserQuestion 暂停/恢复

1. `AskUserQuestionTool` 返回 `ToolError::NeedsUserInput`
2. `Event::AskUserQuestion` → 前端展示选项
3. `answer_question` 写入 tool result → `continue_turn_loop`（`inner_turn_at_start` 重置 turn 内预算）
4. `pending_user_question` 时 **不** emit `TurnComplete`；前端 turn-complete 亦跳过 hydrate

### 1.8 用户中断（AbortController）

**核心类型（`interrupt.rs` / `messages.rs`）：**

| 类型 | 说明 |
|------|------|
| `AbortController` | `watch::channel` + `AtomicBool` 快路径；Tauri `interrupt` 与 turn 共享 |
| `InterruptReason` | `UserCancel` · `SubmitInterrupt` · `SiblingError` · `StreamingFallback` |
| `INTERRUPT_MESSAGE` | `[Request interrupted by user]`（user 消息，写入 SQLite） |

**Turn 循环中断处理：**
1. `call_llm_and_execute`：`create_stream` 传入 `cancel_flag`；`run_abort_bridge` 同步到 `StreamingToolExecutor`
2. 流取消：持久化 partial assistant → 补 tool_result → `append_interrupt_message`（SubmitInterrupt 跳过）
3. 工具阶段 abort：`TerminalReason::AbortedTools`
4. Compaction：`interrupt_requested()` 时跳过 LLM 摘要，降级为规则截断

**TerminalReason：** `AbortedStreaming` · `AbortedTools` · `Completed`

### 1.9 Turn 状态机

`TurnContext`：turn_number、inner_turn、**inner_turn_at_start**、max_inner_turns、pending_approvals

续跑（`approve_tool` / `deny_tool` / `answer_question` → `continue_turn_loop`）时 `inner_turn` 设为已有 assistant 条数（segment 索引），`inner_turn_at_start` 同步，**预算**按 `inner_turn - inner_turn_at_start` 计。

`approve_tool` 经 `execute_one_user_approved` 执行，不再二次 `check_permissions`。

`TurnState`：Ready → Streaming → ExecutingTools → WaitingApproval → Done / Error

内层循环：tool_calls → 执行（可在 SSE 流未结束前完成）→ 追加 tool 消息 → 直至无 tool call 或达 max_inner_turns

### 1.10 流式 Tool 调度（StreamingToolDispatch）

`turn_loop::call_llm_and_execute` 在 `create_stream` **之前**创建 `StreamingToolExecutor`，并传入 `on_tool_call` 回调：

| 权限结果 | 行为 |
|----------|------|
| Allow | 立即 `add_tool`；emit `ToolCallRequest`（running）；流中 poll `ToolCallResult` |
| Ask | 写入 `pending_specs` / `pending_tools`；**不** early `add_tool`；UI 显示 pending |
| Deny | 记录 `denied_specs`；流末注入 `PermissionDenied` tool_result |

**事件顺序（每个 tool）：**
1. `ToolUseStarted` → `ToolInputDelta`（raw JSON 片段）
2. arguments 完整 → `ToolInputComplete`（parsed `input` + `needs_approval`）
3. Allow 时 → `ToolCallRequest` + 流中 `ToolCallResult`
4. 流结束 → 按 `handled_ids` 去重；流末 `ToolCallRequest` 对已在 running/done 的 tool **幂等**

**参数解析：** 统一经 `message_bridge::parse_tool_call_input` → `novel_deepseek::parse_tool_arguments`；失败 fallback `{}`。

**中断：** `AbortController` 触发时 `executor.discard()`（`StreamingFallback`）；partial assistant 持久化但不执行未完成 tool。

Fork 子 Agent：`run_subagent_async` 发出 `SubAgentStarted` / `SubAgentComplete`（含 task_preview）；LogIntegrityChecker 同步路径经 `run_forked_agent_inner` 亦发事件。分析类 / GeneralPurpose 子 Agent 工具流 `event_tx = None`（无逐 tool UI）。

### 1.11 模块索引

| 模块 | 职责 |
|------|------|
| `interrupt` | `AbortController`、`InterruptReason`、abort 常量 |
| `messages` | 中断 user 消息、synthetic tool_result、`is_synthetic_message` |
| `prompt_loader` | 加载 `prompt/agents/*.md`，`format_fork_task` |
| `dynamic_context` | `build_dynamic_context`、`format_activated_skill_block`、skill reference 解析/加载、memory/progress 加载 |
| `subagent_overflow` | 子 Agent 400/length 极简部分报告 |
| `hooks` | PostToolUse 匹配、LogIntegrityChecker task 生成 |
| `message_bridge` | ChatMessage ↔ LlmChatMessage；`parse_tool_call_input` |
| `turn_loop` | inner loop、compaction、**StreamingToolDispatch**、hook drain、approve/deny/answer |

---

## 2. 事件流

`Event` 枚举（engine → novel-server → Tauri）：

| 事件 | 用途 |
|------|------|
| ContentBlockDelta | 流式文本/思考增量 |
| ToolUseStarted | Tool 块出现（名称 + id） |
| ToolInputDelta | arguments JSON 片段（raw） |
| ToolInputComplete | arguments 完整；parsed input + needs_approval |
| ToolCallRequest | 工具调用（含 needs_approval；流末幂等） |
| ToolCallProgress | 工具执行进度 |
| ToolCallResult | 工具执行结果 |
| AskUserQuestion | 创作分歧问答 |
| TurnStart / TurnComplete | Turn 生命周期 + token；暂停态（pending 工具/问答）不 TurnComplete |
| AssistantSegmentComplete | 单段 LLM 结束；可选 `fork_run_id` 供 overlay 分段 |
| SubAgentStarted / SubAgentComplete | 子 Agent（SubAgentComplete 含 agent_id 供前端识别） |
| Error | 错误（含 recoverable 标志） |
