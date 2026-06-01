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
- `total_turns`、`api_call_count`、`metadata_json`（JSON，存 session 级元数据）

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

**Session：** 创建、查询、状态更新、token 累加（`accumulate_session_tokens` 同时递增 `api_call_count` 并刷新 `last_active_at`）。用户对话轮数由 `sync_user_turn_count` 独立写入（不刷新时间戳）。列表按 `project_root` 过滤、`last_active_at` 降序。

**Message：** 插入、按 turn range 查询、Compaction 后 `replace_session_messages` 全量替换。

**Metadata：** `invoked_skill_ids` 与 `read_skill_reference_paths` 存于 session `metadata_json`，resume / compaction rebuild 时恢复。

**Todos / Fork transcript：** TodoWrite 经 upsert 持久化；子 Agent 经 `fork_runs` + `fork_messages`（与 parent messages 分离）。

**API 配置：** 已从 DB 迁移至 agent 级 `api_config.json`。

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
| title | 可选标题（首条用户消息自动写入前 50 字） |
| status | active / archived 等 |
| model | 最后一次 API 调用使用的模型 |
| last_active_at | 最后一次 LLM API 调用结束或中断的时间（`accumulate_session_tokens` / `touch_last_active_at`）；切换会话 resume **不**更新 |
| created_at | 会话创建时间 |
| total_turns | 用户对话轮数（每条用户消息 +1） |
| api_call_count | LLM API 调用次数（inner loop / 子 Agent 计费统计） |

（不含 `total_cost_usd`；费用由 token 字段按需计算。）

**Token 累加：** `accumulate_session_tokens` 更新 token 三类计数、递增 `api_call_count` 并刷新 `last_active_at`；**不**修改 `total_turns`。用户轮数由 `sync_user_turn_count` 写入（**不**刷新 `last_active_at`）。`update_session_status` 仅改 status。

**`last_active_at` 更新规则：**

| 场景 | 是否更新 |
|------|----------|
| LLM 正常返回（有/无 usage 元数据） | ✅ `accumulate_session_tokens` 或 `touch_last_active_at` |
| 流式输出被用户中断 | ✅ 中断处理分支（有 usage 则 accumulate，否则 touch） |
| LLM 已返回、工具执行阶段按 Esc | ❌ 不额外更新（时间戳停在 LLM 结束时刻） |
| 本轮尚未发起任何 API 即中断 | ❌ |
| `resume_session` / 仅切换查看会话 | ❌ |
| `sync_user_turn_count` | ❌ |

**迁移：** 旧库首次打开时 `ensure_api_call_count_column` 将原 `total_turns`（实为 API 计数）迁入 `api_call_count`，并按 messages 中 user 消息重算 `total_turns`。

### 1.6 Compaction 持久化协作

压缩完成后 `novel-core::turn_loop` 调用 `replace_session_messages`，将内存中已压缩的 messages 写回 DB。`invoked_skill_ids` 同步更新，保证 refresh 后 resume 行为一致。
