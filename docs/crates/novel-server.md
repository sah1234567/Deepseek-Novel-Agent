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

- `config: Arc<RwLock<AppConfig>>` — 切换作品时 `write()` 更新 `active_project`
- `spawn_engine_loop(AgentEngine, cmd_rx, config)` — engine 与 config 共享 Arc
- `command_context(app_handle)` → `CommandContext { cmd_tx, config, app_handle, abort_controller, current_message_id }`
- `spawn_event_forwarder(app, message_id)` → 将 `Event` 经 `emit_core_event` 推送到前端

### 1.3 EngineCommand

| 命令 | 说明 |
|------|------|
| `SendMessage` | 用户消息 + 可选 `event_tx` |
| `ApproveTool` / `DenyTool` | 工具批准/拒绝 + `event_tx` 续跑 turn |
| `AnswerQuestion` | AskUserQuestion 回答 + `event_tx` 续跑 |
| `ForkSubAgent` | 手动 fork 子 Agent（已移除 strategy 参数） |
| `GetStatus` | 返回 `AppStatus`（含 `activeWorkName`） |
| `SetPermissionMode` | 切换权限模式 |
| `ResumeSession` | 恢复历史会话（替换 engine） |
| `CreateSession` | 当前作品下新建 session（替换 engine） |
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
  "projectRoot": "/path/to/works/my-novel",
  "activeWorkName": "my-novel",
  "sessionCacheHit": 0,
  "sessionCacheMiss": 0,
  "sessionCompletion": 0,
  "todos": [{ "todoId", "content", "status" }]
}
```

`projectInitialized`：`knowledge/` 或 `AGENTS.md` 存在；`projectRoot` / `activeWorkName` 来自当前 `AppConfig.active_project`。

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
| `resume_session` | 恢复会话 | SettingsPanel |
| `list_sessions` | 当前作品会话列表 | SettingsPanel |
| `get_session_messages` | 历史 hydrate | `useAgent` |
| `init_novel_project` | 当前作品 scaffold | SettingsPanel |
| `list_project_files` / `read_project_file` | 文件树 | FileTreePanel |
| `update_session_todo` | Todo 状态 | TodoPanel |
| `get_api_config` / `set_api_config` | 全局 API（json） | SettingsPanel |
| `fork_sub_agent` | 手动子 Agent | 调试 |

`create_session` **不再**接受 `project_root` 参数；切换作品用 `open_work` / `create_work`。

### 1.7 作品切换流程

```
create_work(name):
  work_path → init_project_scaffold（若目录不存在）
  → SwitchProjectAndCreateSession
    → config.write().set_active_project
    → AgentEngine::new(engine_config_for(work))
    → emit session-resumed

open_work(name):
  校验目录存在 → 同上
```

`ensure_work_under_works` 防止路径逃逸 `works/`。

### 1.8 前端事件（events.rs）

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

`sub-agent-started` 转发 `agentType`；前端 `useAgent` 设置 `activeSubAgent`，StatusBar 显示运行中标签（GeneralPurpose →「自定义 Subagent 运行中…」）。`sub-agent-complete` 清除状态。`forkRunId` 非空时分段 finalize overlay，否则 finalize 主聊天。

前端 `useAgent`：`input_complete` 写入 `parsedInput`；流末 `ToolCallRequest` 不将 running/done 重置为 pending。

### 1.9 API 配置 IPC

- `get_api_config` — 读 `{agent_root}/.novel-agent/api_config.json`；Key 脱敏为 `••••••••`；未配置返回错误
- `set_api_config` — 写入全局 json（`novel_config::save_agent_api_config`）
- Engine `init_llm` 优先级：`DEEPSEEK_API_KEY` > json > `ChatClient::from_env` > offline mock

旧版 per-work `state.db` 内 `api_config` 表已移除，新写入统一走全局 `api_config.json`。

### 1.10 前端构建衔接

| 阶段 | 命令 | 结果 |
|------|------|------|
| 开发 | `cargo tauri dev` | Vite HMR @ `http://localhost:5173` |
| 打包 | `cargo tauri build` | `ui/dist/` 静态资源 |

**打包注意：** 须随应用部署 `templates/`、`skills/` 目录（与开发态相同 agent 根布局），否则 setup 失败。
