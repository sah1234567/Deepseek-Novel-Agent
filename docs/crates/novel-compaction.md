# novel-compaction — 上下文压缩

> 所属项目: [Novel Agent](../../README.md)

---

## 1. 业务逻辑

### 1.1 ContextManager

- 监控上下文预算（默认 100万 × 0.8 = 80万触发）
- `check_budget(message_refs)` → `CompactionDecision::NoAction` / `ShouldCompact`
- `retain_policy()` → `RetainPolicy`

### 1.2 Session 重建式压缩（主路径）

**ReAct 切分 `partition_messages`：**
- `summarize_to` — 摘要 API 前缀 `messages[0..summarize_to]`（KV cache 复用）
- `retain_from` — 保留最近 N 轮完整 user turn 链（默认 5）

**摘要生成（KV cache 优先）：**
- 在 `messages[0..summarize_to]` 末尾追加 `[压缩摘要请求]` user 消息
- 单次 API 调用，**不**替换 system、**不**重复序列化 middle_text
- `build_summary_trailing_user_prompt()` 定稿提示词（无数字软约束；硬截断见 `summary_max_chars`）
- 失败降级：`rule_based_summary` + `[规则摘要]`

**Session 重建 `rebuild_session_messages`：**
1. 刷新 system（Progress、Memory≤4KB、INDEX、AGENTS、Skill 摘要）
2. `[激活 Skill]` user（去重 invoked skill id，有则必选）
3. `[会话历史摘要]` user（~1 万字结构化摘要）
4. 最近 5 轮完整 ReAct（保留 tool_calls；与摘要相邻）

**Level 1：** retain 区 tool_result 内旧章节正文压缩（`apply_level1_on_compaction_messages`）。

**Level 2：** `apply_level2_knowledge` 压缩知识库演化日志表格。**非引擎自动触发**——Agent 通过 `KnowledgeDerive(compress_logs=true)` 自主调用。底层复用 `novel_knowledge::compress_evolution_table`。

**Level 3：** 预留（当前无实现）。

**Level 4 兜底 `apply_level4_compaction`：** 重建后仍超 80% window 时递减 retain 轮数。

### 1.3 RetainPolicy 默认值

| 字段 | 默认 |
|------|------|
| recent_chapters_full | 3 |
| recent_react_turns | 5 |
| summary_max_chars | 10000 |
| summary_max_output_tokens | 16384 |

### 1.4 持久化

压缩完成后 `novel-core::compact_and_sync` 调用 `Database::replace_session_messages` 写回 DB。`invoked_skill_ids` 持久化在 session `metadata_json`。

### 1.5 Token 估算

`estimate_tokens(content)` — tiktoken-rs cl100k_base；不可用时字符数 ÷ 4。
