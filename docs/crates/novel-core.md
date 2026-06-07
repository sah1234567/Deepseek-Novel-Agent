# novel-core — Agent 循环与 Fork 子 Agent

> 所属项目: [Novel Agent](../../README.md)

---

## 1. 业务逻辑

### 1.1 AgentEngine — 主引擎

`AgentEngine` 管理完整的小说创作会话。创建时加载作品配置与 Skill、构建 23 个工具的 ToolRegistry、`assemble_system_prompt` 拼装 system（静态 + 动态段），**立即** persist `(0,0)` system 与 `system_static_frozen` metadata。`resume` 从 SQLite 加载**冻结** system（禁止 `build_initial_prompt` 覆盖），`turn_number` 从已有消息推导。

每条用户消息 `turn_number += 1`；每次 LLM API 返回经 `Database::accumulate_session_tokens` 累计 token 与 `api_call_count`。主循环为 `handle_message → run_inner_turn_loop`，每轮 LLM 流式调用 + 流中 Tool 调度 → `drain_subagent_jobs` → 下一轮或结束。

**Token 记账（四类 DB 字段）：**

| 字段 | 规则 |
|------|------|
| `cache_hit_tokens` / `cache_miss_tokens` / `completion_tokens` | 各自独立 `+=` 本次 API 对应值（主 Agent 与 SubAgent 均参与） |
| `context_tokens` | 主 Agent 本次 API 的 `hit+miss+comp` 快照（SubAgent 计费不覆盖） |

内存侧 `last_context_tokens` 供 compaction 阈值判断。StatusBar 经 `get_app_status` **30s** 轮询 + turn 结束 `onTurnComplete` / `permission-mode-changed` / `tool-call-request`(phase=result) 刷新；会话切换由 invoke 调用方 refresh；`session-tokens-updated` 局部 patch token 字段。

**`message/`：** `convert` / `repair` / `display` / `llm` / `missing_blocks` — ChatMessage ↔ 落库 JSON ↔ LLM API；tool chain repair 失败打 `warn!`。

SubAgent 与主 Agent 共用 `EngineShared.session_llm`（`SessionLlmSnapshot`，含 UI model override），防止 flash/pro 混用导致 prefix cache 失效。主会话 `ChatClient` 由 `engine/session_llm::build_chat_client` 统一构造；`turn/loop` 在 `AgentEngine.llm` 为空时惰性创建，per-turn model override 会先写快照并强制重建客户端。

**公共 API：**

| 方法 | 说明 |
|------|------|
| `new` / `resume` | 新建或恢复会话 |
| `handle_message` | 用户消息，驱动完整的 inner turn loop |
| `fork(agent_type, task)` | 构建子 Agent 上下文（`[system, task]`）；执行须经 `subagent_queue` + `drain_subagent_jobs`（测试/直接 API） |
| `approve_tool` / `deny_tool` | 批准/拒绝待确认工具，批准后可续跑 turn |
| `answer_question` | AskUserQuestion 回答后继续 turn |
| `status_snapshot()` | 返回 `EngineStatus` 供前端轮询 |
| `session_id()` / `list_session_todos()` | 会话 ID 与 TodoWrite 列表（`novel-server` 读状态用） |

### 1.2 Fork 子 Agent

**`EngineShared` 子 Agent 相关字段：** `subagent_queue`、`session_llm`、`drain_in_progress`；`EngineStatus.hook_running` 与之同义（历史字段名，覆盖全部 subagent drain）。详见 [FRAMEWORK.md](../../FRAMEWORK.md)。

**硬约束：** 子 Agent 从 `messages[0]`（system prompt）fork，禁止嵌套 fork。`sub_agent_count > 0` 时 `fork()` 与 `ForkSubAgent` 工具均返回 `NestedForkProhibited`。

**触发路径：**

| 路径 | 主会话 inject | Transcript |
|------|---------------|------------|
| `ForkSubAgent` 工具 | 一条摘要（`[子 Agent 完成: {type}]`） | `fork_messages` + UI overlay |
| PostToolUse 自动触发 | **不 inject** | `fork_messages` + UI overlay |

工具路径：本批 subagent 并行 spawn → join → 按 `subagent_queue` FIFO 逐条注入摘要报告。父 system prompt 不变，DeepSeek prefix cache 可命中 `[m0]`。子 Agent 的消息数组仅 `[system_prompt, task_message]` 2 条。

子 Agent 报告末尾须含 `## 接下来（主 Agent 必读）` 自然语言建议，主 Agent 据此自行决定后续操作。

**子 Agent 类型（4 种）：** PlanAuditor、KnowledgeAuditor、ChapterCraftAnalyzer、GeneralPurpose。`AgentType::parse()` 支持 PascalCase / kebab-case 统一解析。

写章分两层审计：细纲写完后 Fork PlanAuditor（计划质量审计），正文写完后 Fork KnowledgeAuditor + ChapterCraftAnalyzer（执行忠实度 + 文笔一致性）。PlanAuditor 有独立的 `prompt/agents/plan-auditor.md`，工具集含 PlotGraph/ForeshadowTracker/Corkboard 等 10 个只读工具。

GeneralPurpose 的 `task` 即主 Agent 编写的完整自定义 prompt；预定义类型则嵌入 `prompt/agents/*.md` 全文 + 运行时约束。

### 1.3 Agent 类型与工具

详细 prompt 来自 `prompt/agents/*.md`（`include_str!` 编译期嵌入）。主 Agent 读不到这些 prompt 文件——子 Agent 须在报告末尾输出 `## 接下来（主 Agent 必读）`。

| Agent | max_react_loops | 工具概要 |
|-------|-----------------|----------|
| **PlanAuditor** | 30 | Read/Grep/PlotGraph/ForeshadowTracker/CharacterSearch/TrackingQuery/RelationQuery/Corkboard/Tail/Stats（只读） |
| KnowledgeAuditor | 40（settings 可覆盖） | Read/Tail/Grep/CharacterSearch/PlotGraph/TrackingQuery/RelationQuery/ForeshadowTracker（只读） |
| ChapterCraftAnalyzer | 25 | Read/Tail/Grep/CharacterSearch/Stats/TrackingQuery/RelationQuery |
| **GeneralPurpose** | 20 | Read/Write/Edit/Glob/Grep/CharacterSearch/PlotGraph/Tail/Stats/InvokeSkill/ImpactAnalysis/TodoWrite/WebSearch；无 ForkSubAgent，无 Bash |

写章分两层审计：细纲后 Fork PlanAuditor（计划质量），正文后同批 Fork KnowledgeAuditor + ChapterCraftAnalyzer（执行忠实度 + 文笔一致性）。Workflow（策划/写章/改稿/写后）经 **InvokeSkill** 加载。GeneralPurpose 含 Write/Edit/WebSearch，sandbox 路径校验。

### 1.4 System Prompt 与动态上下文

**静态层：** `prompt/system.md` 经 `include_str!()` 编译期嵌入。`prompt/autonomous-writing.md` 为无人值守规则正文；`prompt/permission-mode-enter.md` / `permission-mode-exit.md` 为中途切换前后缀。**新会话 / 压缩重建**且 Unattended 时规则写入 system；**会话中途**切到/退出 Unattended 时将对应前缀 **prepend 到下一条用户消息**（单条 user），不修改 `messages[0]`。

**权限模式切换门禁：** 输出阶段或 turn 未完整结束（待答题 / 待审批）时 `set_permission_mode` 立即拒绝；仅 turn 正常结束或中断后且 idle 时允许。`EngineStatus.turn_in_progress` / 前端 `AppStatus.turnInProgress` 与 `is_turn_in_progress()` 对齐。

**Session 级模式持久化：** `metadata_json.permission_mode` 在新建会话、每次 `apply_permission_mode_change` 与 `resume`（metadata 缺失时自消息推断或 settings 回填并写回）时写入。子 Agent hook batch 临时 Auto/Normal **不写** metadata。

**中途切换注入（与开启 Turn 同构）：** 切换模式仅设置 `pending_permission_user_prefix`（不写 messages）；作者下一条消息走正常 `handle_message` 开启新 Turn，`prepend_permission_notice(prefix, author)` 合并为单条 user。注入类型由 `plan_mode_transition` 决定：**Dual**（非 Unattended→Unattended 且 system 无 autonomous：enter 头 + autonomous 正文）、**Single**（Unattended→其他：exit 正文）、**None**（含 Normal/Plan/Auto 互切、system 已有 autonomous）。Plan/Auto/Normal 工具策略由 `novel-tools` 处理，无额外 user prompt。

**用户消息双字段（中途 enter/exit 注入）：** DB `content_json.content` = 发给 LLM 的 merged 全文（前缀 + `\n\n---\n\n` + 作者原文）；`display_content` = 作者原文（仅 UI，前缀不展示）。`stored_to_chat` / resume / compaction 只读 `content`；`stored_message_display_text` 供 transcript IPC，不参与 token 计算，不影响 system KV 前缀稳定性。

**动态上下文：** agents_md、knowledge_index（≤2000 字符）、memory（≤4KB，截断时追加 WARNING）、progress（章节进度 + TodoWrite）、skill_summaries（仅 name+description 摘要，不含 body；**压缩时读盘刷新**）。

**Skill 加载：** Agent 级 `skills/` + 可选作品级 `works/{名}/skills/`（同 id 覆盖）；system prompt 只含摘要（压缩时重读目录），body 经 InvokeSkill 按需加载，references 经 Read 渐进打开。

**读盘经济：** `system.md` §2.3 + `novel-tools` pipeline 硬限（knowledge/** >80 行拒绝注入）；Grep 使用 ripgrep 后端；Read 256KB 硬限。

**术语：** **Session Turn**（`turn_number`）= 用户一条消息及其完整 inner loop；**ReAct loop** = Turn 内单次 LLM→工具循环；**API 调用**（`api_call_count`）= 一次 LLM 请求，一次 Session Turn 可含多次。达 Subagent ReAct 上限时注入提醒 + report-only 收尾轮（禁 tool），非硬截断。

**压缩后序列（API 工作集）：** system（AGENTS/Workspace metadata 冻结 + Index/Memory/Progress/**Skills 摘要** 读盘刷新）→ `[上下文刷新]` user（Skill 全文 + 会话摘要）→ 最近 5 轮 ReAct。压缩 **mid-turn** 不新开 Session Turn。

**KV cache：** system 中 AGENTS/Workspace 前缀跨压缩字节不变；Index/Memory/Progress/Skills 摘要与 `[上下文刷新]` 每次压缩更新。Resume 使用 DB 完整 system 快照。

### 1.5 断路器

工具停滞保护：`get_remaining_results` 连续 10 次迭代无进展 → abort 所有剩余工具。

### 1.6 KnowledgeAuditor Hook（opt-in）

`default_hook_config()` 返回**空** `post_tool_use`。用户可在 `settings.json` 启用 PostToolUse matcher 后，将 KnowledgeAuditor 任务入队 `subagent_queue`（`parent_tool_call_id: None`），由 `drain_subagent_jobs` 异步执行（仅扫描本次 Write/Edit 遗漏）。

**写后流程（prompt 强制，非引擎硬编码）：** 写章分两层审计——细纲 Write + 追踪文件更新后 Fork PlanAuditor（计划质量审计）；正文 Write + 收尾后**同批 Fork 2 项** Subagent（KnowledgeAuditor + ChapterCraftAnalyzer，执行忠实度 + 文笔一致性），按报告 Edit 后再宣告完成。PostToolUse Hook 仅做轻量扫描，不能替代完整写章收尾 Fork。

### 1.7 AskUserQuestion 暂停/恢复

`AskUserQuestionTool` 返回 `NeedsUserInput` 时，前端展示问答面板，turn 暂停。用户回答后 `answer_question` 写入 tool result，`continue_turn_loop` 续跑（inner turn 预算按当前 turn 内已消耗量计算）。`pending_user_question` 非空时不 emit `TurnComplete`。

### 1.8 用户中断（AbortController）

`AbortController` 提供 `AtomicBool` 快路径 + `watch::channel` 广播。中断原因：`UserCancel`（Esc 键）、`SubmitInterrupt`（发送新消息中断当前流）。

中断时：取消 SSE 流 → 持久化 partial assistant → 补充缺失 tool_result → drain 请求保持 token 计数准确。用户主动中断 emit `TurnComplete`（`was_interrupted`），**不** emit `Event::Error`。Compaction 期间中断则跳过 LLM 摘要，降级为规则截断。

### 1.9 Turn 状态机

`TurnContext` 仅跟踪 **ReAct inner turn** 预算（`inner_turn` / `inner_turn_at_start` / `max_inner_turns`）；待审批工具统一在 `AgentEngine.pending_tools`（`!pending_tools.is_empty()` 时 turn 暂停）。

续跑（approve/deny/answer → `continue_turn_loop`）时 `inner_turn` 设为已有 assistant 条数，预算按 `inner_turn - inner_turn_at_start` 计，避免长会话耗尽 80 次上限。`approve_tool` 不再二次 `check_permissions`。

### 1.10 流式 Tool 调度（StreamingToolDispatch）

LLM 流式输出期间，arguments JSON 完整即开始权限检查与调度：

| 权限结果 | 行为 |
|----------|------|
| Allow | 立即入队执行；流中 poll 结果推 UI |
| Ask | 写入 pending，等待用户 approve/deny |
| Deny | 流末注入 `PermissionDenied` tool_result |

参数解析统一经 `parse_tool_call_input`；失败 fallback `{}`。流结束时按 handled_ids 去重，流末 ToolCallRequest 对已在 running/done 的 tool 幂等。

### 1.11 源码目录（按调用域）

```
src/
  lib.rs, agent.rs, types.rs, error.rs, hooks.rs
  context/          manager, system_prompt, prompt_loader, dynamic_context/
  engine/           types, lifecycle, run_loop, prompt_permission, session, session_llm
  turn/             context, loop/, llm_stream, tool_dispatch, tool_apply, tool_merge
  subagent/         mod (queue/drain), runner, helpers, llm_turn, react, overflow, fork/
  message/          convert, display, llm, repair, missing_blocks
  interrupt/        controller, finalize, abort_map
  permission/       mode_prompt
```

`lib.rs` 对外 re-export 保持稳定：`AgentType`、`FORKABLE_AGENT_TYPE_NAMES`、`AgentEngine`、`Event`/`Op`、权限文案 helper、`AbortController` 等。

---

## 2. 事件流

`Event` 枚举（engine → Tauri 前端）：

| 事件 | 用途 |
|------|------|
| ContentBlockDelta | 流式文本/思考增量 |
| ToolUseStarted / ToolInputDelta / ToolInputComplete | Tool 参数流式到达（完整后含 parsed input + needs_approval） |
| ToolCallRequest / ToolCallProgress / ToolCallResult | 工具执行全生命周期 |
| AskUserQuestion | 创作分歧问答 |
| TurnStart / TurnComplete | Turn 生命周期 + token 统计；pending 工具/问答时不 TurnComplete |
| AssistantSegmentComplete | 单段 LLM 结束；可选 `fork_run_id` 供 overlay 分段 |
| SubAgentStarted / SubAgentComplete | 子 Agent 生命周期（含 agent_type、task_preview、parent_tool_call_id；前端推断 hook/tool） |
| SubAgentStreamDelta / SubAgentToolUpdate | 子 Agent 流式正文与工具，实时推送前端 overlay |
| CompactionProgress | 压缩进度（`compaction-progress` → CompactionBanner；payload：`started` / `generating-summary` / `rebuilding-session` / `done` / `failed`） |
| Error | 可恢复错误（`turn-complete` `phase: "error"`）；用户主动中断不走此路径 |
