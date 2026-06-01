# novel-server — Tauri IPC 桥梁

> 所属项目: [Novel Agent](../../README.md)

---

## 1. 业务逻辑

### 1.1 架构

```
React (ui/) ──invoke/listen──► src-tauri/commands.rs
                                    │
                                    ▼
                         novel-server::tauri
                           engine_loop (单任务队列)
                                    │
                                    ▼
                              AgentEngine
```

**无 `Mutex<AgentEngine>`：** 所有命令经 `mpsc::UnboundedSender<EngineCommand>` 入队，`engine_loop` 串行处理，避免 IPC 持锁与审批死锁。

**启动校验（`main.rs` setup）：** 创建 `works/`、`.novel-agent/`；校验 `templates/` 存在；若 `works/default` 不存在则自动 scaffold；注册 `AppState`。

### 1.2 AppState

`AppConfig` 以 `Arc<RwLock<_>>` 共享于 engine loop 与 Tauri commands 之间，切换作品时更新 `active_project`。`CommandContext` 封装 engine 命令通道、config、abort_controller，供所有 Tauri command 使用。Engine 产生的 `Event` 经 `spawn_event_forwarder` 推送到前端。

### 1.3 EngineCommand

| 命令 | 说明 |
|------|------|
| `SendMessage` | 用户消息 + 可选 `event_tx` |
| `ApproveTool` / `DenyTool` | 工具批准/拒绝 + `event_tx` 续跑 turn |
| `AnswerQuestion` | AskUserQuestion 回答 + `event_tx` 续跑 |
| `ForkSubAgent` | 手动 fork 子 Agent（已移除 strategy 参数） |
| `GetStatus` | 返回 `AppStatus`（含 `activeWorkName`） |
| `SetPermissionMode` | 切换权限模式 |
| `ResumeSession` | 恢复历史会话（替换 engine；`abort_controller.clear()`） |
| `CreateSession` | 当前作品下新建 session（替换 engine；`abort_controller.clear()`） |
| `SwitchProjectAndCreateSession` | 切换 `active_project` + 新建 session（`create_work` / `open_work`） |

### 1.4 AppStatus

```json
{
  "sessionId": "...",
  "permissionMode": "normal|plan|auto|unattended",
  "hookRunning": false,
  "pendingUserQuestion": false,
  "turnNumber": 0,
  "projectInitialized": true,
  "hasInterruptibleToolInProgress": false,
  "activeWorkName": "my-novel",
  "sessionCacheHit": 0,
  "sessionCacheMiss": 0,
  "sessionCompletion": 0,
  "contextTokens": 0,
  "todos": [{ "todoId", "content", "status" }]
}
```

`projectInitialized`：`knowledge/` 或 `AGENTS.md` 存在；`activeWorkName` 来自当前 `AppConfig.active_project`。

### 1.5 WorkSummary

`list_works` 扫描 `{agent_root}/works/` 子目录：

| 字段 | 说明 |
|------|------|
| name | 作品文件夹名 |
| path | 绝对路径 |
| initialized | 存在 `AGENTS.md` 或 `knowledge/` |

### 1.6 Tauri Commands（src-tauri/commands.rs）

| Command | 说明 | 前端 Hook |
|---------|------|-----------|
| `send_message` | 发送聊天 | `useAgent.sendMessage` |
| `interrupt` | 中断当前 turn | Esc / 发送中断 |
| `approve_tool` / `deny_tool` | 工具批准/拒绝 | ChatPanel |
| `answer_question` | AskUserQuestion | 问答 UI |
| `get_app_status` | 轮询状态 | `useAppStatus` |
| `set_permission_mode` | 权限模式 | SettingsPanel |
| `list_works` | 作品列表 | StatusBar 下拉 |
| `create_work(name)` | 新建作品 + 切换 + 新 session | StatusBar |
| `open_work(name)` | 打开作品 + 新 session | StatusBar |
| `create_session` | 当前作品新建 session | StatusBar `+` |
| `resume_session` | 恢复/切换会话 | StatusBar 下拉 · SettingsPanel |
| `list_sessions` | 当前作品会话列表（`last_active_at` 降序） | StatusBar · SettingsPanel |
| `get_session_messages` | 历史 hydrate | `useAgent`（`session-resumed` / `sessionId` 变化） |
| `init_novel_project` | 当前作品 scaffold | SettingsPanel |
| `list_project_files` / `read_project_file` | 文件树 | FileTreePanel |
| `update_session_todo` | Todo 状态 | TodoPanel |
| `get_api_config` / `set_api_config` | 全局 API（json） | SettingsPanel |
| `fork_sub_agent` | 手动子 Agent | 调试 |

`create_session` **不再**接受 `project_root` 参数；切换作品用 `open_work` / `create_work`。

**Tauri invoke 参数命名：** 前端使用 camelCase，Rust 命令参数为 snake_case。示例：

```typescript
invoke("resume_session", { sessionId });
invoke("get_session_messages", { sessionId: null });
invoke("update_session_todo", { todoId, status });
invoke("approve_tool", { toolCallId });
```

### 1.7 SessionSummary（list_sessions 返回值）

与 [novel-state §1.5](novel-state.md#15-sessionsummary) 一致。前端 StatusBar 标签用 `total_turns`（对话轮数）与 `last_active_at`（相对时间）；SettingsPanel 额外展示 `api_call_count`。

### 1.8 作品切换

`create_work` / `open_work` 均触发 `SwitchProjectAndCreateSession`：更新 `active_project`、创建新 AgentEngine、emit `session-resumed`。路径经 `ensure_work_under_works` 校验防止逃逸 `works/`。新建作品时若目录不存在则先 scaffold。

### 1.9 前端事件（events.rs）

所有 tool 相关 UI 事件均经 **`tool-call-request`** 通道，以 `phase` 区分：

| phase | 字段 | Core Event |
|-------|------|------------|
| `start` | toolCallId, toolName | ToolUseStarted |
| `input_delta` | toolCallId, delta | ToolInputDelta |
| `input_complete` | toolCallId, toolName, input, needsApproval | ToolInputComplete |
| （无 phase） | toolCallId, toolName, input, needsApproval | ToolCallRequest（流末幂等） |
| `progress` | toolCallId, status, description | ToolCallProgress |
| `result` | toolCallId, content | ToolCallResult |

其他 Tauri 事件：

| Tauri 事件 | 来源 |
|------------|------|
| `stream-chunk` | ContentBlockDelta |
| `ask-user-question` | AskUserQuestion |
| `turn-complete` | TurnComplete / TurnStart / Error |
| `session-resumed` | create/open work、create/resume session |
| `permission-mode-changed` | SetPermissionMode |
| `sub-agent-started` / `sub-agent-complete` | 子 Agent（含 GeneralPurpose；payload 含 `agentType`） |
| `sub-agent-stream` / `sub-agent-tool` | 子 Agent overlay 流式正文与工具 |
| `assistant-segment-complete` | AssistantSegmentComplete（`segmentIndex`；可选 `forkRunId`） |

子 Agent 事件驱动 StatusBar chip 与 SubAgentOverlay：`sub-agent-started` 显示运行中标签，`sub-agent-complete` 清除。`forkRunId` 非空时分段 finalize overlay，否则 finalize 主聊天。

### 1.10 API 配置

`get_api_config` / `set_api_config` 读写 `{agent_root}/.novel-agent/api_config.json`（Key 脱敏）。Engine `init_llm` 优先级：`DEEPSEEK_API_KEY` 环境变量 > json 文件 > 离线 mock。旧版 per-work `state.db` 内 `api_config` 表已移除。

### 1.11 前端构建衔接

| 阶段 | 命令 | 结果 |
|------|------|------|
| 开发 | `cargo tauri dev` | Vite HMR @ `http://localhost:5173` |
| 打包 | `cargo tauri build` | `ui/dist/` 静态资源 |

**打包注意：** 须随应用部署 `templates/`、`skills/` 目录（与开发态相同 agent 根布局），否则 setup 失败。
