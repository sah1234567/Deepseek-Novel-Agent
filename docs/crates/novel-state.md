# novel-state — SQLite 持久化

> 所属项目: [Novel Agent](../../README.md)

---

## 1. 业务逻辑

### 1.1 Database

- r2d2 连接池 + rusqlite，WAL 模式
- `Database::open(path)` 自动 migrations
- 最大连接 8，超时 5s
- **每作品独立 DB：** `{work}/.novel-agent/state.db`

### 1.2 Schema（6 表 + 1 视图）

**schema_version：** 迁移版本号

**sessions：** UUID 主键，project_root, title, status, model, provider
- Token 三类：cache_hit_tokens, cache_miss_tokens, completion_tokens
- `total_turns`、`metadata_json`（JSON，存 session 级元数据）

**messages：** session_id FK，turn_number, sequence, role, content_json
- 消息级 token 三类 + estimated_tokens
- 唯一约束：(session_id, turn_number, sequence)

**checkpoints：** Fork 检查点

**sub_agent_runs：** legacy schema，运行时未使用；新 fork transcript 用 `fork_runs` / `fork_messages`

**fork_runs：** 每次 fork 实例（`source`: `tool` | `hook`）；`report_message_id` 仅工具路径 inject 后有值

**fork_messages：** 子 Agent 内 assistant/tool 消息（与 parent `messages` 分离）；`get_fork_messages(run_id)` 供 IPC/overlay

**session_todos：** 会话级待办（TodoWrite 工具）

**daily_token_stats 视图**

### 1.3 核心操作

**Session：** create_session, get_session, update_session_status, add_session_tokens, list_sessions（按 `project_root` 过滤）

**Message：** insert_message, get_session_messages, **`replace_session_messages`**（Compaction 后全量替换 session 消息，避免 resume 加载未压缩历史）

**Metadata：**
- `get_session_metadata` / `set_session_metadata`
- `get_invoked_skill_ids` / `set_invoked_skill_ids` — 存于 `metadata_json.invoked_skill_ids`，resume 时恢复 InvokeSkill 状态
- `get_read_skill_reference_paths` / `set_read_skill_reference_paths` — 存于 `metadata_json.read_skill_reference_paths`，Read 成功读取 `skills/{id}/references/*.md` 时去重记录，压缩重建时注入 `[激活 Skill]`

**Todos：** upsert_session_todos, list_session_todos

**Checkpoint：** create_checkpoint, get_checkpoint

**Fork transcript：** `create_fork_run`, `insert_fork_message`, `finish_fork_run`, `get_fork_run`, `get_fork_messages`

**API（legacy）：** `get_api_config` / `set_api_config` — 已从 DB 迁移至 agent 级 `api_config.json`；DB 内 `api_config` 表已移除。

### 1.4 成本定价

- `COST_HIT_PER_M: 0.014`
- `COST_MISS_PER_M: 0.14`
- `COST_COMPLETION_PER_M: 0.28`
- `compute_cost_usd(hit, miss, completion)`

TodoWrite 通过 `upsert_session_todos` 写入；`dynamic_context::load_progress` 与 `get_app_status` 读取展示。

### 1.5 SessionSummary

`list_sessions(project_root, limit)` 返回：

| 字段 | 说明 |
|------|------|
| id | 会话 UUID |
| title | 可选标题 |
| status | active / archived 等 |
| last_active_at | 最近活跃时间 |
| total_turns | 累计 turn 数 |

（不含 `total_cost_usd`；费用由 token 字段按需计算。）

### 1.6 Compaction 持久化协作

压缩完成后 `novel-core::turn_loop` 调用 `replace_session_messages`，将内存中已压缩的 messages 写回 DB。`invoked_skill_ids` 同步更新，保证 refresh 后 resume 行为一致。
