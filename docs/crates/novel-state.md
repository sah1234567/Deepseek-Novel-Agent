# novel-state — SQLite 持久化

> 所属项目: [Novel Agent](../../README.md)

---

## 1. 业务逻辑

### 1.1 Database

源码按职责拆分 `crates/novel-state/src/db/{mod,migrate,sessions,messages,metadata,fork_runs}.rs`；JSON 列解析失败返回 `rusqlite::Error` 而非静默默认值。

- r2d2 连接池 + rusqlite，WAL 模式
- `Database::open(path)` 自动 migrations（schema v2 含 `message_archive`）
- 最大连接 8，超时 5s
- **每作品独立 DB：** `{work}/.novel-agent/state.db`

### 1.2 Schema（7 表 + schema_version）

**schema_version：** 迁移版本号（当前 v2）

**sessions：** UUID 主键，project_root, title, status, model, provider
- Token 三类：`cache_hit_tokens`, `cache_miss_tokens`, `completion_tokens`（各自独立 `+=`）
- `context_tokens`：主 Agent 最近一次 API 调用的 `hit+miss+comp` 快照（覆盖写入；SubAgent 计费不累加此字段）
- `total_turns`、`api_call_count`、`metadata_json`（JSON，含 `system_static_frozen`、`frozen_*`、`compaction_count` 等）

**messages：** API **工作集**（压缩后仅保留 system + context user + retain ReAct + 新轮次）
- turn/seq 编码：`(0,0)` system → `(0,1)` `[上下文刷新]` user → `turn≥1` 真实对话
- 唯一约束：(session_id, turn_number, sequence)

**message_archive：** 每次压缩前从 `messages` 整表快照（含 `compaction_epoch`）；供 UI 全历史回放，**不进 LLM API**

**fork_runs / fork_messages：** 子 Agent transcript（与 parent `messages` 分离）

**session_todos：** 会话级待办（TodoWrite 工具）

**Legacy cleanup：** 打开 DB 时自动 `DROP` 旧版 `checkpoints` / `sub_agent_runs` / `daily_token_stats`。

### 1.3 核心操作

**Session：** 创建、查询、状态更新。

**Token 与活跃时间（单一 API）：** `accumulate_session_tokens(session_id, hit, miss, completion, model, update_context_snapshot)` 在一次 UPDATE 中：
- 三类 token 各自 `+=`
- 当 `update_context_snapshot == true`（主 Agent）：`context_tokens = hit + miss + completion`（覆盖为本次 API 快照，供 StatusBar「当前上下文」）
- 当 `false`（SubAgent 计费）：跳过 `context_tokens` 更新，保留主 Agent 快照
- `api_call_count += 1`
- 刷新 `last_active_at` 与 `model`

主 Agent 与 SubAgent 的 LLM 调用均经此 API 落库计费；仅主 Agent 更新 `context_tokens`。用户对话轮数由 `sync_user_turn_count` 独立写入（不刷新时间戳；`[上下文刷新]` user 不计入）。

**Message：** 增量 `insert_message`；Compaction **先** `archive_session_messages(epoch)` **再** `replace_session_messages`（工作集全量替换）。

**Archive：** `get_archived_epochs`、`get_archived_messages(session_id, epoch)`、`get_archived_messages_turn_range(session_id, epoch, from, to)`。

**Turn 边界（UI 懒加载）：** `TurnBounds { min_turn, max_turn }`；`get_active_turn_bounds`、`get_archived_turn_bounds`；`has_active_context_refresh`（`(0,1)` user 且 `[上下文刷新]` 前缀）。`get_session_messages(session_id, turn_range)` 供 `novel-core` 与 IPC turn 切片共用。前端 `EVICT_TURNS` 仅卸载内存中的 turn 正文（slot 变 idle 占位），**不删** `messages` / `message_archive`；贴底驻留与 `TAIL_LOADED_TURNS`（6）一致；上滑仍可按 turn 范围 IPC 重载。

**Metadata：** `invoked_skill_ids`、`read_skill_reference_paths`；`system_static_frozen` + `frozen_agents_md` / `frozen_workspace_path` / `system_static_sha256`；`compaction_count`。

**Resume 校验：** 会话 metadata 异常时可运行 `reset-work-databases` 后新建 session。

**Todos：** `list_todos` / `upsert_todos` 仅在 `db::todo` 模块与 `Database` 方法暴露；**不再**从 `novel-state` crate 根 re-export（调用方经 `db` 或 `novel-server` IPC）。

**Fork transcript：** 同前。

### 1.4 SessionSummary

`list_sessions(project_root, limit)` 返回：id、title、status、model、last_active_at、created_at、**total_turns**（用户对话轮，不含 `[上下文刷新]`）、**api_call_count**。

**Token 相关函数：** `accumulate_session_tokens`、`sync_user_turn_count`、`touch_last_active_at`（行为同前）。

**`last_active_at` 更新规则：** LLM 返回/中断时更新；`resume_session` 纯切换不更新。

**迁移：** 无 runtime 合并；库格式异常时用 [reset-work-databases](../../scripts/reset-work-databases.ps1) 清理后新建 session。

### 1.5 Compaction 持久化协作

1. `increment_compaction_count` → epoch
2. `archive_session_messages` — 压缩前整表快照
3. `novel-core` 内存重建后 `replace_session_messages` — 仅工作集
4. `invoked_skill_ids` / reference paths 同步 metadata

**建会话：** `AgentEngine::new` 即 `insert_message(0,0,system)` + 写入静态快照 metadata（非等到首次 compact）。

**清理会话库：** [reset-work-databases](../../scripts/reset-work-databases.ps1) / `.sh` — 见 [README §清理作品会话库](../../README.md#清理作品会话库)。
