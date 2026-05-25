# novel-deepseek — DeepSeek LLM 客户端

> 所属项目: [Novel Agent](../../README.md)
>
> **说明：** 原 `novel-llm` 薄 wrapper 已合并入本 crate（2026-05）。`novel-core` 仅依赖 `novel_deepseek::`。

---

## 1. 业务逻辑

### 1.1 ChatClient — 流式聊天客户端

`ChatClient` 是唯一的 LLM 调用入口，使用 **reqwest** 直连 DeepSeek OpenAI 兼容 API（SSE）。所有 LLM 调用走流式 `/chat/completions`。

**创建方式：**
- `ChatClient::deepseek(api_key, model, api_base)` — 显式指定 Key、模型与 Base URL
- `ChatClient::from_env(model)` — 从环境变量 `DEEPSEEK_API_KEY` 读取 API Key

**Engine 初始化（`novel-core::turn_loop::init_llm`）：**

| 优先级 | 来源 |
|--------|------|
| 1 | 环境变量 `DEEPSEEK_API_KEY` |
| 2 | `{agent_root}/.novel-agent/api_config.json` |
| 3 | `ChatClient::from_env(model)` |
| 4 | 无 Key → `llm = None`，走 `offline_complete` mock |

**核心方法（均为 `&mut self`，因内置 `CacheTracker`）：**
- `create_stream(messages, tools, max_tokens, on_event, on_tool_call, cancel, enable_web_search?)`
  - 返回 `StreamOutcome::Complete` 或 `StreamOutcome::Cancelled`
- `complete_via_stream(...)` — 无工具场景（如 compaction 摘要）
- `offline_complete(messages)` — 离线 mock
- **`web_search(api_key, query, max_results)`** — 静态方法；通过 DeepSeek `web_search_20250305` 服务器端搜索（Anthropic Messages API），供 WebSearch 工具调用
- `cache_tracker()` / `cache_tracker_mut()` — 会话级 cache 统计

**流式 tool call 处理：**
1. 每个 SSE chunk 的 `tool_calls` delta 写入内联 `PendingTool`（按 index 累积 raw arguments 字符串）
2. 名称/id 就绪后 emit `StreamEvent::ToolUseStarted` / `ToolInputDelta`
3. 每次 arguments 追加后，若 `parse_tool_arguments` 成功**且解析后对象非空** → 调用 `on_tool_call`
   - `parse_tool_arguments("")` 返回 `Ok({})`，但空对象表示参数尚未到达（DeepSeek 首 chunk 的 `function.arguments` 为 `""`），此时**不发射**
4. 流结束再对未 callback 的 index 补一次 `try_emit_ready_tool`
5. `drain_pending` 原样输出 raw arguments 字符串（不在协议层 repair JSON）

### 1.2 Tool Schema 与参数解析

**`tool_to_json(name, desc, schema)`** — 将工具 JSON Schema **整包**作为 OpenAI `function.parameters` 传入（非仅拆 `properties`/`required`）。

**`tool_args.rs`：**
- `parse_tool_arguments(raw)` — trim；empty → `{}`；strict JSON，**无 repair 启发式**
- `ToolParseError` — `EmptyArguments` / `InvalidJson`

`novel-core::message_bridge::parse_tool_call_input` 统一消费上述解析，parse 失败 fallback 为 `{}` 并打 warn。

### 1.3 Token 与 Cache 追踪

| 类别 | 说明 |
|------|------|
| Cache Hit | `prompt_tokens_details.cached_tokens` |
| Cache Miss | prompt − hit |
| Completion | `completion_tokens` |

**`CacheTracker`（`cache.rs`）：** 每次 `StreamOutcome::Complete` 时 `record(usage)`；提供 `hit_rate()` 与累计 `CacheStats`。

**`TokenUsage`：** `from_deepseek_usage(hit, miss, completion, …)`、`cache_hit_rate()`、`total_prompt()`。

### 1.4 流事件类型

| 类型 | 说明 |
|------|------|
| `StreamEvent::ContentBlockDelta` | 文本 / 思考增量 |
| `StreamEvent::ToolUseStarted` | tool 名称与 id 就绪 |
| `StreamEvent::ToolInputDelta` | arguments JSON 片段 |
| `StreamEvent::MessageStop` | 流结束 |
| `StreamEvent::StreamError` | 可重试解析错误 |

`ContentBlockKind`：Text · Thinking · ToolCall（engine 侧 mostly Text/Thinking）。

### 1.5 模块索引

| 模块 | 职责 |
|------|------|
| `client.rs` | `ChatClient`、SSE 解析、`PendingTool`、per-tool callback |
| `cache.rs` | `CacheTracker`、`CacheStats` |
| `tool_args.rs` | `parse_tool_arguments`、`ToolParseError` |
| `types.rs` | `LlmChatMessage`、`LlmToolCall`、`StreamOutcome`、`TokenUsage` |
| `config.rs` | 嵌入 `config.toml`、默认 API base |
| `connectivity.rs` | 端点连通性测试（`#[ignore]` live tests） |

---

## 2. 与 novel-core 的接线

```
turn_loop::call_llm_and_execute
  → ChatClient::create_stream(..., on_tool_call = Some(...))
  → StreamingToolDispatch::handle_ready   // parse + 权限 + ToolInputComplete
  → StreamingToolExecutor::add_tool       // Allow 时流中提前执行
  → poll get_completed_results            // 流中增量 ToolCallResult
```

Fork 子 Agent 同样传入 `on_tool_call`，但不 emit UI 事件（`event_tx = None`）。
