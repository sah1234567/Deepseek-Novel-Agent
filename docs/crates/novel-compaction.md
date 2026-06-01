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
- 跳过 `(0,1)` `[上下文刷新]` user（非 turn 边界）
- `summarize_to` — 摘要 API 前缀 `messages[0..summarize_to]`（KV cache 复用）
- `retain_from` — 保留最近 N 轮完整 user turn 链（默认 5），**原样**保留 tool_calls / reasoning

**摘要生成（KV cache 优先）：**
- 在 `messages[0..summarize_to]` 末尾追加 trailing user（正文来自 `prompt/compaction-summary-trailing.md`，`include_str!`）
- 首行 `[压缩摘要请求]`；**不**写入 DB、**不**进入 rebuild
- 失败降级：`rule_based_summary` + `[规则摘要]`

**Session 重建 `rebuild_session_messages`：**
1. system（由 `novel-core::refresh_system_dynamic_sections` 提供：AGENTS/Workspace metadata 冻结 + Index/Memory/Progress/**Skills 摘要** 读盘刷新）
2. **单条** `[上下文刷新]` user（`wrap_context_refresh_user_message`：可选 `## 激活 Skill` + `## 会话历史摘要`）
3. 最近 N 轮完整 ReAct（Level1 可改 retain 区 tool_result 文本）

**Level 4 兜底：** 重建后仍超 80% window 时递减 retain 轮数。

### 1.3 RetainPolicy 默认值

（同前：recent_react_turns=5, summary_max_chars=10000 等）

### 1.4 持久化

1. `archive_session_messages(compaction_epoch)` — 压缩前 UI 全量快照
2. `replace_session_messages` — API 工作集
3. `metadata_json`：`invoked_skill_ids`、`compaction_count`、system 静态快照

### 1.5 Token 估算

`estimate_tokens(content)` — tiktoken-rs cl100k_base；不可用时字符数 ÷ 4。
