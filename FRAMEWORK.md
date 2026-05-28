# Novel Agent — 完整架构框架

---

## 1. 架构总览

### 1.1 系统分层

```
React Frontend (ui/src/)
  ChatPanel · StatusBar · TodoPanel · SettingsPanel

Tauri IPC (src-tauri/commands.rs)
  → novel-server::tauri (engine_loop 单任务队列)

Rust Backend (crates/)
  9 个业务 crate + novel-server + src-tauri，单向依赖
```

### 1.2 Agent 根目录与数据归属

```
novel_agent/                          ← resolve_agent_root()（含 skills/ 标记）
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

`AppConfig` 为 `Arc<RwLock<_>>`；`create_work` / `open_work` / `CreateSession` 更新 `active_project` 并重建 engine，文件树与会话列表读当前作品 DB。

### 1.3 Crate 依赖关系

```
novel-server (Tauri IPC)
  └─ novel-core (AgentEngine, Fork, Hook, dynamic_context)
       ├─ novel-deepseek (ChatClient, SSE, cache, tool_args)
       ├─ novel-tools (22 tools, StreamingToolExecutor)
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
| **Fork 即子 Agent** | 从 `messages[0]` fork；预定义类型 `task_message = {embedded prompt} + 约束 + task`；GeneralPurpose 的 `task` 即自定义 prompt 主体 |
| **检查类 Subagent** | KnowledgeAuditor / ChapterCraftAnalyzer 只读报告；**写章/改稿后 system prompt 要求同批 Fork 2 项**（引擎不硬编码，由 prompt 约束）；GeneralPurpose 13 工具白名单；KnowledgeAuditor 另可 PostToolUse Hook opt-in（轻量扫描） |
| **Workflow Skill** | `novel-planning` / `chapter-writing` / `revision` / `post-chapter-checklist` 经 InvokeSkill 加载 SOP；`## 本阶段完成后` 自然语言后续指引 |
| **Session 重建压缩** | 超阈值时刷新 system + `[会话历史摘要]` + 5 轮 ReAct + Skill 全文。前端进度横幅 + 用户驱动断路器（3 次失败后可重试或新建会话） |
| **中断与 token 估算** | `AbortController` 立即断开 SSE 流，drain 请求估算 prompt_tokens 保持 session 总数准确 |
| **Skill 二级加载** | Agent 级 `skills/` + 可选作品级 `works/{名}/skills/`（同 id 作品覆盖 Agent）；摘要进 system prompt，正文经 InvokeSkill |
| **脚手架仅磁盘** | `templates/` 缺失 → `TemplatesNotFound`；无 embed fallback |
| **压缩 DB 同步** | Compaction 后 `replace_session_messages`；`invoked_skill_ids` 与 `read_skill_reference_paths` 存 `metadata_json` |
| **Hook opt-in** | `default_hook_config` 默认空；用户 settings 可启用 PostToolUse Hook |
| **单队列 Engine** | 所有 IPC 经 `engine_loop` 串行 |
| **流式 Tool 早执行** | arguments 完整即 dispatch；Allow 提前 `add_tool`；Ask 等 approve；Deny 流末 error result |
| **读盘经济** | `prompt/system.md` §2.3 + `novel-tools` tool result pipeline（`read_economy` 硬限）：knowledge/** >80 行拒绝注入；Grep ripgrep 后端；usage_hint 软引导 |
| **Turn 续跑预算** | `continue_turn_loop` 设置 `inner_turn_at_start`，inner turn 上限按**当前 turn 内**迭代计，避免长会话 assistant 条数耗尽 80 次 cap |
| **Segment 分段 UI** | `AssistantSegmentComplete { segment_index, fork_run_id }`：主聊天与子 Agent overlay 各 finalize 独立气泡（CoT + 正文） |

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
invoke('send_message')
  → EngineCommand::SendMessage
  → handle_message_with_events
    → compact_with_events（必要时压缩，emit CompactionProgress 事件到前端）
    → run_inner_turn_loop → LLM 流式 + 流中 Tool 调度
    → drain_pending_hooks（PostToolUse 自动触发 KnowledgeAuditor subagent，source=`hook`）
    → drain_pending_forks（ForkSubAgent 工具路径 → 并行 run_subagent_async → join → inject 一条报告）
  → Event::TurnComplete（`pending_tools` 或 `pending_user_question` 非空时不发送）
```

**Turn 暂停：** 待批准 Write/Edit、待回答 AskUserQuestion 时 inner loop 返回 `Completed` 但不 TurnComplete；`approve_tool` / `deny_tool` / `answer_question` 清空 pending 后 `continue_turn_loop`。

**ForkSubAgent 工具路径**完成后 `inject_sub_agent_report` 向主会话注入 **一条** `[子 Agent 完成: {type}]` 摘要（含 `fork_run_id` UI 元数据）；完整 transcript 写入 `fork_messages`，**不**进入主 LLM prompt。PostToolUse 路径（KnowledgeAuditor subagent，`source=hook`）**故意不 inject**，避免污染主会话上下文；UI 经 StatusBar / SubAgentOverlay 查看。

主 Agent 读工具路径报告末尾 **`## 接下来（主 Agent 必读）`** 自然语言建议，自行决定后续 Invoke/Fork/Edit。

**流式 Tool 时序（`call_llm_and_execute`）：**

```
SSE delta
  → ToolUseStarted / ToolInputDelta（UI: phase start / input_delta）
  → arguments JSON 完整 → on_tool_call
  → parse_tool_call_input + check_permissions
  → ToolInputComplete（UI: phase input_complete, parsedInput）
  → Allow: add_tool + 流中 poll ToolCallResult
  → Ask: pending_tools，等 approve_tool
  → 流结束：按 id 去重，仅处理 remainder；流末 ToolCallRequest 幂等
```

### 2.2 作品与会话

```
StatusBar 作品下拉:
  list_works → open_work(name) / create_work(name)
    → SwitchProjectAndCreateSession（更新 active_project + 新 session）

StatusBar 会话 +:
  create_session()  ← 当前作品下新建 session

SettingsPanel:
  resume_session(id)  ← 当前作品 DB 内恢复
  list_sessions()
```

### 2.3 Fork 子 Agent

**术语：** **Session Turn**（`turn_number`）= 用户一条消息跑完 inner loop；**ReAct loop**（`inner_turn` / `max_react_loops`）= Turn 内单次 LLM→工具循环。Subagent `max_react_loops` 耗尽时注入提醒 + report-only 收尾轮，非硬截断。

**触发方式：**

| 路径 | 说明 |
|------|------|
| `ForkSubAgent` 工具 | 主会话 LLM 调用；入队 `fork_queue` → `drain_pending_forks` 同步 join → `inject_sub_agent_report`（一条摘要 + `fork_run_id`） |
| PostToolUse 自动触发 | settings 启用 matcher 后，PostToolUse 管道入队 **KnowledgeAuditor subagent**（`source=hook`）；`run_knowledge_auditor_hook` 同步执行；**不 inject** 主会话 |
| Tauri `fork_sub_agent` | 调试 / IPC 手动 fork（fire-and-forget，不经 sync inject） |

**LLM 上下文隔离：** 子 Agent 内 assistant/tool 消息仅存 `fork_messages` + 前端 overlay；主 LLM 仅见工具路径的一条报告摘要。Compaction / `replace_session_messages` 不合并 `fork_messages`。

**消息结构：**

```
fork → [system_prompt, format_fork_task(agent_type, task)]
  → run_subagent_async / run_forked_agent（persist fork_messages + scoped UI events）
  → 工具路径: inject_sub_agent_report → 主会话一条 user 摘要
  → hook 路径: 无 inject；SubAgentComplete { fork_run_id } 供 UI
```

**双轨 prompt（`prompt_loader::format_fork_task`）：**

| agentType | task_message 组成 |
|-----------|-------------------|
| 预定义（Checker/Analyzer 等） | `prompt/agents/{name}.md` 全文 + 运行时约束 + `---` + 简短 task |
| **GeneralPurpose** | `prompt/agents/general_purpose.md` 短壳 + 运行时约束 + `---` + **## 自定义任务** + 完整 task |

Agent prompt 文件：`prompt/agents/*.md`（`include_str!` 编译期嵌入）。类型解析统一经 `AgentType::parse`；可 fork 列表见 `FORKABLE_AGENT_TYPE_NAMES`（KnowledgeAuditor、ChapterCraftAnalyzer、GeneralPurpose）。

**GeneralPurpose 权限：** 精选工具白名单（Read/Write/Edit/Glob/Grep/CharacterSearch/PlotGraph/Tail/Stats/InvokeSkill/ImpactAnalysis/TodoWrite/WebSearch）；无 ForkSubAgent 工具（禁止嵌套 fork），无 Bash。

**代码审查清单（禁止替 Agent 决策）：**

- 无 `allow_chapter_write` / `validate_chapter_write` / 按路径禁止 Write
- 无 Write 后默认自动 KnowledgeAuditor 入队（除非用户 opt-in hooks）
- 无 `fork_handoff` / `## 引擎交接` 引擎解析驱动 Fork 链
- Subagent 只读约束仅在 fork 角色 prompt + 工具白名单，不限制主 Agent

**UI 事件：** fork 实例经 scoped Tauri 事件更新前端 overlay（不 append 主 Chat `messages`）：

| 事件 | 用途 |
|------|------|
| `sub-agent-started` / `sub-agent-complete` | StatusBar chip；含 `forkRunId` |
| `sub-agent-stream` / `sub-agent-tool` | SubAgentOverlay 实时 transcript |
| `assistant-segment-complete` | 含可选 `forkRunId`；主聊天或 overlay 分段 finalize |
| `get_fork_messages` IPC | resume / 完成后从 SQLite 回放 |

工具路径：主 Chat `SubAgentReportCard`「查看详情」→ overlay。Hook 路径：StatusBar 可点击 chip + 完成 banner「查看详情」。

### 2.4 System Prompt 与动态上下文

**初始（`build_initial_prompt`）：**

| 段 | 来源 |
|----|------|
| 静态层 | `prompt/system.md`（含读盘经济 §2.3、写后 2 项 Fork 规范） |
| AGENTS.md | 作品根 |
| INDEX | `knowledge/INDEX.md`（≤2000 字） |
| Skills | `skills/` 摘要 only |
| Memory | `memory/`（≤4KB） |
| Progress | 章节数 + TodoWrite |

**Compaction 后：** 刷新 system → `[激活 Skill]` → `[会话历史摘要]` + 最近 ReAct（摘要与 ReAct 相邻连贯）。

### 2.5 前端状态与 IPC

**AppStatus：** `sessionId`, `permissionMode`, `activeWorkName`, `projectRoot`, `turnNumber`, `todos`, token 三分类累计 + `sessionTotalTokens`（= hit+miss+comp）, `projectInitialized`, `hookRunning`, `hasInterruptibleToolInProgress`, …

| 领域 | 主要 Command |
|------|----------------|
| 作品 | `list_works`, `create_work`, `open_work` |
| 会话 | `create_session`, `resume_session`, `list_sessions`, `get_session_messages` |
| 聊天 | `send_message`, `interrupt`, `approve_tool`, `deny_tool`, `answer_question` |
| 文件 | `list_project_files`, `read_project_file`（当前 active 作品） |
| 配置 | `get_api_config`, `set_api_config`（全局 json） |
| 脚手架 | `init_novel_project` |

### 2.6 用户中断

`AbortController` 贯穿 LLM 流与工具：Esc → `UserCancel`（立即断开 SSE），发送消息 → `SubmitInterrupt`（不添中断消息）。

中断链路：
1. `interrupt()` 直接设置 `AtomicBool`（不经 engine_loop 队列，零延迟）
2. `tokio::select!` 在 20ms 内检测并取消 SSE 流
3. 部分 assistant 消息持久化，缺失 tool_result 补填
4. **drain 请求**：独立发送 `stream=false, max_tokens=1` 请求，利用相同 messages 的 prefix cache 命中，获取 `prompt_tokens` 保持 session 总 token 准确
5. 原始请求的 `cache_hit/miss/completion` 已丢失，drain 的三类拆分仅为估算

详见 novel-core §1.8 与 novel-deepseek `drain_usage_background` 文档注释。

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

**开发验编译（可选，不产出 exe）：**

```powershell
cd novel_agent
cargo check --workspace
```

**开发运行：**

```powershell
cd ui; npm install; cd ..
cargo tauri dev
```

**Release 构建（下载后本地安装）：**

```powershell
cd ui; npm install; cd ..
cargo tauri build
```

```powershell
.\scripts\check.ps1
.\scripts\run_tests.ps1
```

打包发布时须随应用部署 `templates/` 与 `skills/`（与开发态相同布局）。
