# novel-logging — 结构化日志与调试追踪

> 所属项目: [Novel Agent](../../README.md)

---

## 1. 业务逻辑

### 1.1 初始化

- `init_logging(project_root)` — 统一入口：人类可读 **stderr** + 可选 JSON 文件层
- `init()` — 等价于 `init_logging(None)`（仅 stderr）
- `init_with_json_log(project_root)` — 等价于 `init_logging(Some(project_root))`（向后兼容）

**默认 filter**（无 `RUST_LOG` 时）：

```
novel_agent=info,novel_core=info,novel_deepseek=info,novel_tools=info,novel_server=info
```

**stderr 与 `cargo tauri dev`：** 与 dev 进程同一终端。默认 filter 为 `info` 时，**终端仅常见**：

- `turn_start` / `turn_complete`（字段：`turn`、`reason`）
- `compaction_start` / `compaction_done`
- `warn!` / `error!`（LLM、工具、DB 等失败）

IPC、LLM 请求、每笔 tool、落库、续跑、子 Agent 等均为 `debug!`，需设置 `RUST_LOG=novel_core=debug,novel_server=debug` 才在终端显示。审计时间线见 `作品根/.novel/logs/session_{id}/agent.jsonl`（与 stderr 级别无关）。

**文件 debug.log：** 当传入 `project_root` 且满足以下任一条件时，追加写入 `{作品根}/.novel/logs/debug.log`（JSON 行）：

- Debug 构建（`cfg(debug_assertions)`）；或
- 环境变量 `NOVEL_DEBUG_LOG=1`

深度排查示例：

```powershell
$env:RUST_LOG = "novel_core=debug,novel_deepseek=debug,novel_tools=debug"
$env:NOVEL_DEBUG_LOG = "1"
cargo tauri dev
```

`RUST_LOG` 会覆盖默认 filter；仅初始化一次（`Once`），启动时需已知作品根才能同时打开文件层。

### 1.2 AuditLogger — 审计日志器

`novel-core` 在 `AgentEngine::new` / `resume` 时为每个会话打开 AuditLogger，写入 `作品根/.novel/logs/session_{id}/`。

**agent.jsonl — 主日志（JSON Lines，每行一个事件）：**

- SessionCreated — 会话创建
- TurnStarted / TurnCompleted — 用户消息触发的 turn 边界
- LlmRequest — 流式 LLM 请求开始
- TokenAudit — 单次 LLM 调用的 cache_hit / cache_miss / completion tokens
- ToolExecuted — 工具名与 success
- Error — 可恢复错误（流式错误、用户中断等）
- LogIntegrityCheckerForked — PostToolUse 钩子触发完整性检查子 Agent
- CompactionTriggered — 上下文 level4 压缩（含 tokens_before）

**token_audit.jsonl：** 仅 TokenAudit 事件副本，便于 jq 汇总。

**replay.jsonl：** API 存在；当前运行路径未全量写入正文（避免体积与隐私），消息落库见 `persist_message` 的 `debug` tracing。

### 1.3 jq 示例

```bash
# 最近一次 turn
jq 'select(.event=="TurnCompleted")' agent.jsonl | tail -1

# 失败工具
jq 'select(.event=="ToolExecuted" and .data.success==false)' agent.jsonl

# Token 汇总
jq 'select(.event=="TokenAudit")' agent.jsonl
```

### 1.4 与 state.db 区分

| 用途 | 路径 |
|------|------|
| 对话持久化、UI 恢复 | `{作品根}/.novel-agent/state.db` |
| 实时调试时间线 | stderr + `debug.log` |
| Turn/工具/Token 审计 | `{作品根}/.novel/logs/session_{id}/agent.jsonl` |

### 1.5 线程安全

AuditLogger 内部用 `Arc<Mutex<File>>` 管理三文件，Mutex poison 时自动恢复。
