# novel-compaction — 上下文压缩

> 所属项目: [Novel Agent](../../README.md)

---

## 1. 业务逻辑

### 1.1 ContextManager

- 监控上下文预算（默认 `context_window_size` 100 万 × `compaction_threshold` 0.8 → 约 80 万 token 触发压缩）
- `check_budget(message_refs)` → `CompactionDecision::NoAction` / `ShouldCompact`
- `retain_policy()` → `RetainPolicy`

### 1.2 Session 重建式压缩（主路径）

**ReAct 切分 `partition_messages`：**
- 跳过 `(0,1)` `[上下文刷新]` user（非 turn 边界）
- `summarize_to` — 摘要 API 前缀 `messages[0..summarize_to]`（KV cache 复用）
- `retain_from` — 保留最近 N 轮完整 user turn 链（默认 5），**原样**保留 tool_calls / reasoning

**摘要生成（KV cache 优先）：**
- 在 `messages[0..summarize_to]` 末尾追加 trailing user（正文来自 `prompt/compaction-summary-trailing.md`，`include_str!`）
- 模板含「上一章衔接锚点」（原文末 3 句 + 细纲场景摘要）和「活跃伏笔」（未来 5 章待回收伏笔 ID）字段，加速压缩后写作恢复
- 首行 `[压缩摘要请求]`；**不**写入 DB、**不**进入 rebuild
- 失败降级：`rule_based_summary` + `[规则摘要]`

**Session 重建 `rebuild_session_messages`：**
1. system（由 `novel-core::refresh_system_dynamic_sections` 提供：AGENTS/Workspace metadata 冻结 + Index/Memory/Progress/**Skills 摘要** 读盘刷新）
2. **单条** `[上下文刷新]` user（`wrap_context_refresh_user_message`：可选 `## 激活 Skill` + `## 会话历史摘要`）
3. 最近 N 轮完整 ReAct（原样保留 tool_calls / reasoning）

**Retain 预算裁剪（`rebuild_session_under_budget`）：** 对重建后会话用 `estimate_tokens` 粗算各条 `content`；若占比仍 ≥ **`compaction_threshold × 0.5`**（默认 0.8×0.5=**40%** window，约 40 万 token），则递减保留的 ReAct 轮数直至低于该比例。

### 1.3 RetainPolicy 默认值

（同前：recent_react_turns=5, summary_max_chars=10000 等）

### 1.4 持久化

1. `archive_session_messages(compaction_epoch)` — 压缩前 UI 全量快照
2. `replace_session_messages` — API 工作集
3. `metadata_json`：`invoked_skill_ids`、`compaction_count`、system 静态快照

### 1.5 Token 估算

`estimate_tokens(content)` — 字符数 ÷ 2 + 1 的启发式（仅用于 retain 裁剪）；触发压缩则用 API 真实 `last_context_tokens`。
