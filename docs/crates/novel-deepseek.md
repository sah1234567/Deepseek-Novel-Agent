# novel-deepseek — DeepSeek LLM 客户端

> 所属项目: [Novel Agent](../../README.md)
>
> **说明：** `novel-core` 仅依赖 `novel_deepseek::`（LLM 客户端与 SSE 解析均在本 crate）。

---

## 1. 业务逻辑

### 1.1 ChatClient — 流式聊天客户端

`ChatClient` 是唯一的 LLM 调用入口，使用 **reqwest** 直连 DeepSeek OpenAI 兼容 API（SSE）。所有 LLM 调用走流式 `/chat/completions`。

**创建方式：**
- `ChatClient::deepseek(api_key, model, api_base, thinking_enabled)` — 显式指定 Key、模型与 Base URL
- `ChatClient::from_env(model)` — 从环境变量 `DEEPSEEK_API_KEY` 读取 API Key（embedded base，`thinking_enabled: true`）
- `ChatClient::from_api_key_or_env(api_key, api_base, model, thinking)` — 有 key 用 `deepseek`，否则 `from_env`

**Engine 主会话客户端（`novel-core::session_llm::build_chat_client` + `turn_loop::init_llm`）：**

| 步骤 | 来源 |
|------|------|
| Key 解析 | `novel_config::resolve_agent_api_key`：env `DEEPSEEK_API_KEY` > `api_config.json` |
| Base | `novel_config::resolve_agent_api_base` |
| 构造 | `ChatClient::from_api_key_or_env`；快照来自 `EngineShared.session_llm`（含 StatusBar model override） |
| 无客户端 | `llm = None` → `offline_complete` mock |

**核心方法：**
- `create_stream` — 流式 LLM 调用，返回 `StreamOutcome::Complete` 或 `Cancelled`；支持 on_event/on_tool_call 回调
- `complete_via_stream` — 无工具场景（compaction 摘要）
- `offline_complete` — 离线 mock
- `web_search` — 静态方法，DeepSeek 服务器端搜索

流式 tool call 在 arguments JSON 完整时立即回调 `on_tool_call`（空对象表示参数尚未到达，不发射）。流结束补发未就绪的 tool。`drain_pending` 原样输出 raw arguments（不在协议层 repair JSON）。

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

`ContentBlockKind`：Text · Thinking · ToolCall；`novel-core::Event` 复用本 enum（流式 delta / 子 Agent overlay）。

