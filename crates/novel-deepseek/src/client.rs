use crate::cache::CacheTracker;
use crate::config::chat_api_base;
use crate::error::LlmError;
use crate::tool_args::parse_tool_arguments;
use crate::types::{
    ContentBlockKind, LlmChatMessage, LlmCompletion, LlmToolCall, StreamEvent, StreamOutcome,
    TokenUsage, WebSearchResult,
};
use futures::StreamExt;
use serde::Deserialize;
use serde_json::Value;
use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Duration;

// Web Search via DeepSeek's `web_search_20250305` server-side tool (Anthropic Messages API).

#[derive(Clone)]
pub struct ChatClient {
    api_key: String,
    api_base: String,
    pub model: String,
    pub thinking_enabled: bool,
    http: reqwest::Client,
    cache_tracker: CacheTracker,
}

// ── Internal SSE types ──────────────────────────────────────────

#[derive(Debug, Deserialize)]
struct StreamChunk {
    choices: Option<Vec<StreamChoice>>,
    usage: Option<UsageInfo>,
}

#[derive(Debug, Deserialize)]
struct StreamChoice {
    #[serde(default, rename = "index")]
    _index: u32,
    delta: StreamDelta,
    finish_reason: Option<String>,
}

#[derive(Debug, Deserialize)]
struct StreamDelta {
    #[serde(rename = "role")]
    _role: Option<String>,
    content: Option<String>,
    #[serde(default)]
    reasoning_content: Option<String>,
    tool_calls: Option<Vec<StreamToolCall>>,
}

#[derive(Debug, Deserialize)]
struct StreamToolCall {
    index: u32,
    id: Option<String>,
    #[serde(rename = "type")]
    _type: Option<String>,
    function: Option<StreamFunction>,
}

#[derive(Debug, Deserialize)]
struct StreamFunction {
    name: Option<String>,
    arguments: Option<String>,
}

#[derive(Debug, Deserialize)]
struct UsageInfo {
    prompt_tokens: Option<u32>,
    completion_tokens: Option<u32>,
    prompt_tokens_details: Option<PromptTokensDetails>,
}

#[derive(Debug, Deserialize)]
struct PromptTokensDetails {
    cached_tokens: Option<u32>,
}

struct PendingTool {
    id: String,
    name: Option<String>,
    arguments: String,
    start_emitted: bool,
    ready_emitted: bool,
}

struct StreamAccumulators {
    content_buf: String,
    reasoning_buf: String,
    pending: HashMap<u32, PendingTool>,
    usage: Option<TokenUsage>,
    stop_reason: Option<String>,
}

impl StreamAccumulators {
    fn new() -> Self {
        Self {
            content_buf: String::new(),
            reasoning_buf: String::new(),
            pending: HashMap::new(),
            usage: None,
            stop_reason: None,
        }
    }

    fn apply_chunk<TF>(
        &mut self,
        chunk: &StreamChunk,
        on_event: &mut impl FnMut(StreamEvent),
        on_tool_call: &mut Option<TF>,
    ) where
        TF: FnMut(LlmToolCall),
    {
        if let Some(u) = &chunk.usage {
            let hit = u
                .prompt_tokens_details
                .as_ref()
                .and_then(|d| d.cached_tokens)
                .unwrap_or(0) as i64;
            let miss = u.prompt_tokens.unwrap_or(0) as i64 - hit;
            let comp = u.completion_tokens.unwrap_or(0) as i64;
            self.usage = Some(TokenUsage::from_deepseek_usage(hit, miss, comp, 0));
        }
        let Some(choices) = &chunk.choices else {
            return;
        };
        for choice in choices {
            if let Some(sr) = &choice.finish_reason {
                self.stop_reason = Some(sr.clone());
            }
            let delta = &choice.delta;
            if let Some(rc) = &delta.reasoning_content {
                if !rc.is_empty() {
                    self.reasoning_buf.push_str(rc);
                    on_event(StreamEvent::ContentBlockDelta {
                        index: 0,
                        delta: rc.clone(),
                        kind: ContentBlockKind::Thinking,
                    });
                }
            }
            if let Some(c) = &delta.content {
                self.content_buf.push_str(c);
                on_event(StreamEvent::ContentBlockDelta {
                    index: 0,
                    delta: c.clone(),
                    kind: ContentBlockKind::Text,
                });
            }
            if let Some(tcs) = &delta.tool_calls {
                for tc in tcs {
                    let idx = tc.index;
                    let entry = self.pending.entry(idx).or_insert_with(|| PendingTool {
                        id: String::new(),
                        name: None,
                        arguments: String::new(),
                        start_emitted: false,
                        ready_emitted: false,
                    });
                    if let Some(id) = &tc.id {
                        entry.id = id.clone();
                    }
                    if let Some(func) = &tc.function {
                        if let Some(name) = &func.name {
                            entry.name = Some(name.clone());
                        }
                        if !entry.start_emitted && entry.name.is_some() && !entry.id.is_empty() {
                            on_event(StreamEvent::ToolUseStarted {
                                index: idx,
                                tool_call_id: entry.id.clone(),
                                name: entry.name.clone().unwrap_or_default(),
                            });
                            entry.start_emitted = true;
                        }
                        if let Some(args) = &func.arguments {
                            if !args.is_empty() && !entry.id.is_empty() {
                                on_event(StreamEvent::ToolInputDelta {
                                    tool_call_id: entry.id.clone(),
                                    delta: args.clone(),
                                });
                            }
                            entry.arguments.push_str(args);
                        }
                        if let Some(cb) = on_tool_call {
                            ChatClient::try_emit_ready_tool(entry, cb);
                        }
                    }
                }
            }
        }
    }
}

// ── ChatClient ──────────────────────────────────────────────────

impl ChatClient {
    pub fn deepseek(api_key: &str, model: &str, api_base: &str, thinking_enabled: bool) -> Self {
        Self {
            api_key: api_key.to_string(),
            api_base: api_base.trim_end_matches('/').to_string(),
            model: model.to_string(),
            thinking_enabled,
            http: reqwest::Client::new(),
            cache_tracker: CacheTracker::default(),
        }
    }

    pub fn cache_tracker(&self) -> &CacheTracker {
        &self.cache_tracker
    }

    pub fn cache_tracker_mut(&mut self) -> &mut CacheTracker {
        &mut self.cache_tracker
    }

    pub fn from_env(model: &str) -> Result<Self, LlmError> {
        let api_key = std::env::var("DEEPSEEK_API_KEY").map_err(|_| LlmError::MissingApiKey)?;
        Ok(Self::deepseek(&api_key, model, &chat_api_base(), true))
    }

    /// Explicit API key wins; otherwise [`Self::from_env`] (`DEEPSEEK_API_KEY` + embedded base).
    pub fn from_api_key_or_env(
        api_key: Option<&str>,
        api_base: &str,
        model: &str,
        thinking_enabled: bool,
    ) -> Option<Self> {
        api_key
            .map(|key| Self::deepseek(key, model, api_base, thinking_enabled))
            .or_else(|| Self::from_env(model).ok())
    }

    /// Non-streaming `max_tokens=1` probe for prompt token breakdown after stream cancel.
    pub async fn measure_prompt_usage(
        &self,
        messages: &[LlmChatMessage],
        tools: &[(String, String, Value)],
        initial: Option<TokenUsage>,
    ) -> Option<TokenUsage> {
        let oai = Self::to_openai_messages(messages);
        let tool_defs = Self::build_tools(tools);
        let (tx, rx) = tokio::sync::oneshot::channel();
        self.drain_usage_background(&oai, &tool_defs, initial, tx, 1)
            .await;
        rx.await.ok().flatten()
    }

    // ── Message / tool conversion ─────────────────────────────

    pub fn to_openai_messages(messages: &[LlmChatMessage]) -> Vec<Value> {
        messages.iter().map(msg_to_json).collect()
    }

    pub fn build_tools(tools: &[(String, String, Value)]) -> Vec<Value> {
        tools
            .iter()
            .map(|(name, desc, schema)| tool_to_json(name, desc, schema))
            .collect()
    }

    // ── Streaming ─────────────────────────────────────────────

    pub async fn create_stream<TF, EE>(
        &mut self,
        messages: &[LlmChatMessage],
        tools: &[(String, String, Value)],
        max_tokens: u32,
        mut on_event: EE,
        mut on_tool_call: Option<TF>,
        cancel: Option<Arc<AtomicBool>>,
    ) -> Result<StreamOutcome, LlmError>
    where
        EE: FnMut(StreamEvent),
        TF: FnMut(LlmToolCall),
    {
        let openai_msgs = Self::to_openai_messages(messages);
        let tool_defs = Self::build_tools(tools);

        let mut body = serde_json::json!({
            "model": self.model,
            "messages": openai_msgs,
            "max_tokens": max_tokens,
            "stream": true,
            "stream_options": { "include_usage": true },
        });
        if self.thinking_enabled {
            body["thinking"] = serde_json::json!({ "type": "enabled" });
        }
        if !tool_defs.is_empty() {
            body["tools"] = Value::Array(tool_defs.clone());
        }

        let url = format!("{}/chat/completions", self.api_base);
        let resp = self
            .http
            .post(&url)
            .header("Authorization", format!("Bearer {}", self.api_key))
            .header("Content-Type", "application/json")
            .header("Accept", "text/event-stream")
            .json(&body)
            .send()
            .await
            .map_err(|e| LlmError::Api(e.to_string()))?;

        if !resp.status().is_success() {
            let status = resp.status();
            let text = resp.text().await.unwrap_or_default();
            let preview: String = text.chars().take(500).collect();
            if status.as_u16() == 429 {
                tracing::warn!(%status, body_preview = %preview, "deepseek_rate_limited");
                return Err(LlmError::RateLimited(text));
            }
            if status.as_u16() == 400 && text.to_lowercase().contains("maximum context length") {
                tracing::warn!(%status, body_preview = %preview, "deepseek_context_length_exceeded");
                return Err(LlmError::ContextLengthExceeded { body: text });
            }
            tracing::warn!(%status, body_preview = %preview, "deepseek_http_error");
            return Err(LlmError::Api(format!("HTTP {status}: {text}")));
        }

        let byte_stream = resp.bytes_stream();
        futures::pin_mut!(byte_stream);

        let mut acc = StreamAccumulators::new();
        let mut line_buf = String::new();

        // For background drain on cancel
        let drain_self = self.clone();
        let drain_msgs = Self::to_openai_messages(messages);
        let drain_tools = tool_defs;

        let return_cancelled = |acc: StreamAccumulators| {
            Ok(drain_self.cancelled_stream_outcome(acc, &drain_msgs, &drain_tools))
        };

        loop {
            let chunk_result = if let Some(ref flag) = cancel {
                tokio::select! {
                    r = byte_stream.next() => r,
                    () = Self::wait_cancel(Arc::clone(flag)) => {
                        return return_cancelled(acc);
                    }
                }
            } else {
                byte_stream.next().await
            };

            let Some(chunk) = chunk_result else { break };
            let chunk_bytes = chunk.map_err(|e| LlmError::Api(e.to_string()))?;
            let chunk_str = String::from_utf8_lossy(&chunk_bytes);
            line_buf.push_str(&chunk_str);

            // Process complete lines
            while let Some(pos) = line_buf.find('\n') {
                let line = line_buf[..pos].trim().to_string();
                line_buf = line_buf[pos + 1..].to_string();

                match crate::stream_parse::parse_sse_line(&line) {
                    crate::stream_parse::SseLine::Skip => continue,
                    crate::stream_parse::SseLine::Done => break,
                    crate::stream_parse::SseLine::InvalidJson(e) => {
                        on_event(StreamEvent::StreamError {
                            message: format!("JSON parse: {e}"),
                            retryable: true,
                        });
                        continue;
                    }
                    crate::stream_parse::SseLine::Data(json) => {
                        let chunk: StreamChunk = match serde_json::from_value(json) {
                            Ok(c) => c,
                            Err(e) => {
                                on_event(StreamEvent::StreamError {
                                    message: format!("JSON parse: {e}"),
                                    retryable: true,
                                });
                                continue;
                            }
                        };
                        acc.apply_chunk(&chunk, &mut on_event, &mut on_tool_call);
                        if let Some(flag) = &cancel {
                            if flag.load(Ordering::SeqCst) {
                                return return_cancelled(acc);
                            }
                        }
                    }
                }
            }
        }

        on_event(StreamEvent::MessageStop {
            stop_reason: acc.stop_reason.clone(),
        });

        if let Some(ref mut cb) = on_tool_call {
            for entry in acc.pending.values_mut() {
                Self::try_emit_ready_tool(entry, cb);
            }
        }
        let tool_calls = Self::drain_pending(&mut acc.pending);
        let completion = Self::make_completion(
            acc.content_buf,
            acc.reasoning_buf,
            tool_calls,
            &acc.usage,
            acc.stop_reason.clone(),
        );
        if let Some(u) = &completion.usage {
            self.cache_tracker.record(u);
        }
        Ok(StreamOutcome::Complete(completion))
    }

    fn try_emit_ready_tool<TF>(entry: &mut PendingTool, cb: &mut TF)
    where
        TF: FnMut(LlmToolCall),
    {
        if entry.ready_emitted {
            return;
        }
        let Some(name) = entry.name.clone() else {
            return;
        };
        // Don't emit until the arguments JSON is actually complete.
        // parse_tool_arguments returns Ok({}) for an empty string, but
        // an empty object means the real arguments haven't arrived yet.
        let parsed = match parse_tool_arguments(&entry.arguments) {
            Ok(v) => v,
            Err(_) => return, // invalid JSON, wait for next chunk
        };
        if entry.id.is_empty() || parsed.as_object().is_none_or(|o| o.is_empty()) {
            return;
        }
        entry.ready_emitted = true;
        cb(LlmToolCall {
            id: entry.id.clone(),
            name,
            arguments: entry.arguments.clone(),
        });
    }

    // ── Helpers ────────────────────────────────────────────────

    fn drain_pending(pending: &mut HashMap<u32, PendingTool>) -> Vec<LlmToolCall> {
        let mut tools = Vec::new();
        for (_, p) in pending.drain() {
            if let Some(name) = p.name {
                tools.push(LlmToolCall {
                    id: p.id,
                    name,
                    arguments: p.arguments,
                });
            }
        }
        tools
    }

    fn cancelled_stream_outcome(
        &self,
        mut acc: StreamAccumulators,
        drain_msgs: &[Value],
        drain_tools: &[Value],
    ) -> StreamOutcome {
        let tool_calls = Self::drain_pending(&mut acc.pending);
        let partial = Self::make_completion(
            acc.content_buf,
            acc.reasoning_buf,
            tool_calls,
            &acc.usage,
            acc.stop_reason.clone(),
        );
        let (tx, rx) = tokio::sync::oneshot::channel();
        let drain_self = self.clone();
        let usage_snapshot = acc.usage.clone();
        let msgs = drain_msgs.to_vec();
        let tools = drain_tools.to_vec();
        tokio::spawn(async move {
            let _ = drain_self
                .drain_usage_background(&msgs, &tools, usage_snapshot, tx, 30)
                .await;
        });
        StreamOutcome::Cancelled {
            partial,
            background_usage: rx,
        }
    }

    fn make_completion(
        content: String,
        reasoning: String,
        tool_calls: Vec<LlmToolCall>,
        usage: &Option<TokenUsage>,
        stop_reason: Option<String>,
    ) -> LlmCompletion {
        LlmCompletion {
            content: if content.is_empty() {
                None
            } else {
                Some(content)
            },
            reasoning_content: if reasoning.is_empty() {
                None
            } else {
                Some(reasoning)
            },
            tool_calls,
            usage: usage.clone(),
            stop_reason,
        }
    }

    async fn wait_cancel(cancel: Arc<AtomicBool>) {
        while !cancel.load(Ordering::SeqCst) {
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
    }

    /// Send a separate non-streaming request with the same messages to get
    /// `prompt_tokens` so the session's total input-token count stays correct.
    ///
    /// **This is NOT a recovery of the original request's usage.**
    /// The original request's `cache_hit`, `cache_miss`, and `completion_tokens`
    /// are **lost** — the SSE stream was cancelled before the usage chunk
    /// arrived and DeepSeek has no "replay usage" endpoint.
    ///
    /// What this function produces is **drain's own** consumption:
    ///
    /// | field              | value                       |
    /// |--------------------|-----------------------------|
    /// | `prompt_tokens`    | ≈ original (same messages)  |
    /// | `cache_hit`        | drain's (original primed cache → inflated) |
    /// | `cache_miss`       | drain's (under-estimated)   |
    /// | `completion_tokens`| 1 (drain's, not original's) |
    ///
    /// Only `prompt_tokens` (= hit + miss) is trustworthy. The three-class
    /// breakdown is drain's own tiny consumption being added to the DB so the
    /// session counter moves forward — the original turn's true breakdown is
    /// gone. Use the SSE `usage` chunk when available; drain is a last resort
    /// so interrupted turns record something rather than nothing.
    async fn drain_usage_background(
        &self,
        messages: &[Value],
        tools: &[Value],
        initial: Option<TokenUsage>,
        tx: tokio::sync::oneshot::Sender<Option<TokenUsage>>,
        timeout_secs: u64,
    ) {
        let mut body = serde_json::json!({
            "model": self.model, "messages": messages,
            "max_tokens": 1, "stream": false,
        });
        if self.thinking_enabled {
            body["thinking"] = serde_json::json!({ "type": "enabled" });
        }
        if !tools.is_empty() {
            body["tools"] = Value::Array(tools.to_vec());
        }
        let url = format!("{}/chat/completions", self.api_base);
        let drain = async {
            let resp = self
                .http
                .post(&url)
                .header("Authorization", format!("Bearer {}", self.api_key))
                .json(&body)
                .send()
                .await
                .ok()?;
            let json: Value = resp.json().await.ok()?;
            let u = json.get("usage")?;
            let hit = u
                .get("prompt_tokens_details")
                .and_then(|d| d.get("cached_tokens"))
                .and_then(|v| v.as_i64())
                .unwrap_or(0);
            let prompt = u.get("prompt_tokens").and_then(|v| v.as_i64()).unwrap_or(0);
            let comp = u
                .get("completion_tokens")
                .and_then(|v| v.as_i64())
                .unwrap_or(0);
            Some(TokenUsage::from_deepseek_usage(hit, prompt - hit, comp, 0))
        };
        let final_usage = tokio::time::timeout(Duration::from_secs(timeout_secs), drain)
            .await
            .unwrap_or(initial);
        let _ = tx.send(final_usage);
    }

    pub async fn complete_via_stream(
        &mut self,
        messages: &[LlmChatMessage],
        tools: &[(String, String, Value)],
        max_tokens: u32,
        cancel: Option<Arc<AtomicBool>>,
    ) -> Result<LlmCompletion, LlmError> {
        match self
            .create_stream(
                messages,
                tools,
                max_tokens,
                |_| {},
                None::<fn(LlmToolCall)>,
                cancel,
            )
            .await?
        {
            StreamOutcome::Complete(c) => Ok(c),
            StreamOutcome::Cancelled { .. } => Err(LlmError::Cancelled),
        }
    }

    pub fn offline_complete(messages: &[LlmChatMessage]) -> LlmCompletion {
        let last_user = messages
            .iter()
            .rev()
            .find(|m| m.role == "user")
            .map(|m| m.content.as_str())
            .unwrap_or("");
        LlmCompletion {
            content: Some(format!("收到：{last_user}")),
            tool_calls: vec![],
            usage: Some(TokenUsage::from_deepseek_usage(0, 10, 5, 0)),
            reasoning_content: None,
            stop_reason: None,
        }
    }

    /// Perform a web search via DeepSeek's `web_search_20250305` server-side tool.
    /// Uses the Anthropic Messages API format (separate endpoint from chat completions).
    /// Returns a list of `(title, url, snippet)` tuples.
    pub async fn web_search(
        api_key: &str,
        query: &str,
        max_results: usize,
    ) -> Result<Vec<WebSearchResult>, LlmError> {
        let url = crate::config::web_search_messages_url();
        let body = serde_json::json!({
            "model": "deepseek-chat",
            "max_tokens": 1024,
            "system": "You are a web search assistant. Search for the given query and return factual results.",
            "messages": [{
                "role": "user",
                "content": format!("Perform a web search for: {query}")
            }],
            "tools": [{
                "type": "web_search_20250305",
                "name": "web_search",
                "max_uses": 1
            }],
            "stream": false,
        });

        let client = reqwest::Client::builder()
            .timeout(std::time::Duration::from_secs(30))
            .build()
            .map_err(|e| LlmError::Api(e.to_string()))?;
        let resp = client
            .post(&url)
            .header("Content-Type", "application/json")
            .header("x-api-key", api_key)
            .header("anthropic-version", "2023-06-01")
            .json(&body)
            .send()
            .await
            .map_err(|e| LlmError::Api(e.to_string()))?;
        let status = resp.status();
        let text = resp.text().await.unwrap_or_default();
        if !status.is_success() {
            let preview: String = text.chars().take(500).collect();
            tracing::warn!(%status, body_preview = %preview, "deepseek_web_search_http_error");
            return Err(LlmError::Api(format!("web_search HTTP {status}: {text}")));
        }

        let json: serde_json::Value = serde_json::from_str(&text)
            .map_err(|e| LlmError::Api(format!("web_search JSON: {e}")))?;

        let mut results = Vec::new();
        if let Some(blocks) = json.get("content").and_then(|c| c.as_array()) {
            for block in blocks {
                if block.get("type").and_then(|t| t.as_str()) == Some("web_search_tool_result") {
                    if let Some(tool_result) = block.get("content").and_then(|c| c.as_array()) {
                        for item in tool_result.iter().take(max_results) {
                            let title = item
                                .get("title")
                                .and_then(|t| t.as_str())
                                .unwrap_or("")
                                .to_string();
                            let result_url = item
                                .get("url")
                                .and_then(|u| u.as_str())
                                .unwrap_or("")
                                .to_string();
                            let snippet = item
                                .get("snippet")
                                .and_then(|s| s.as_str())
                                .unwrap_or("")
                                .to_string();
                            if !title.is_empty() {
                                results.push(WebSearchResult {
                                    title,
                                    url: result_url,
                                    snippet,
                                });
                            }
                        }
                    }
                }
            }
        }
        Ok(results)
    }
}

// ── JSON helpers ────────────────────────────────────────────────

fn msg_to_json(m: &LlmChatMessage) -> Value {
    let mut obj = serde_json::json!({ "role": m.role.clone(), "content": m.content.clone() });
    if m.role == "assistant" {
        if let Some(rc) = &m.reasoning_content {
            if !rc.is_empty() {
                obj["reasoning_content"] = Value::String(rc.clone());
            }
        }
    }
    if let Some(id) = &m.tool_call_id {
        obj["tool_call_id"] = Value::String(id.clone());
    }
    if let Some(tcs) = &m.tool_calls {
        obj["tool_calls"] = Value::Array(
            tcs.iter()
                .map(|tc| {
                    serde_json::json!({
                        "id": tc.id, "type": "function",
                        "function": { "name": tc.name, "arguments": tc.arguments }
                    })
                })
                .collect(),
        );
    }
    obj
}

fn tool_to_json(name: &str, desc: &str, schema: &Value) -> Value {
    let mut func = serde_json::json!({
        "name": name,
        "parameters": schema.clone(),
    });
    if !desc.is_empty() {
        func["description"] = Value::String(desc.to_string());
    }
    serde_json::json!({ "type": "function", "function": func })
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn from_api_key_or_env_uses_explicit_key_and_snap() {
        let client = ChatClient::from_api_key_or_env(
            Some("test-key"),
            "https://api.example.com/v1",
            "deepseek-chat",
            false,
        )
        .expect("explicit key");
        assert_eq!(client.api_key, "test-key");
        assert_eq!(client.model, "deepseek-chat");
        assert_eq!(client.api_base, "https://api.example.com/v1");
        assert!(!client.thinking_enabled);
    }

    #[test]
    fn from_api_key_or_env_none_without_env_returns_none() {
        let saved = std::env::var("DEEPSEEK_API_KEY").ok();
        std::env::remove_var("DEEPSEEK_API_KEY");
        let result = ChatClient::from_api_key_or_env(None, "https://api.example.com/v1", "m", true);
        if let Some(key) = saved {
            std::env::set_var("DEEPSEEK_API_KEY", key);
        }
        assert!(result.is_none());
    }

    #[test]
    fn offline_complete_echoes_user() {
        let msgs = vec![LlmChatMessage {
            role: "user".into(),
            content: "你好".into(),
            tool_call_id: None,
            tool_calls: None,
            reasoning_content: None,
        }];
        let r = ChatClient::offline_complete(&msgs);
        assert!(r.content.unwrap().contains("你好"));
    }

    #[test]
    fn build_tools_non_empty() {
        let tools = ChatClient::build_tools(&[(
            "Read".into(),
            "Read file".into(),
            json!({"type": "object", "properties": {}, "required": []}),
        )]);
        assert_eq!(tools.len(), 1);
    }

    #[test]
    fn build_tools_preserves_full_schema() {
        let schema = json!({
            "type": "object",
            "properties": { "path": { "type": "string" } },
            "required": ["path"]
        });
        let tools = ChatClient::build_tools(&[("Read".into(), "Read".into(), schema.clone())]);
        assert_eq!(tools[0]["function"]["parameters"], schema);
    }

    #[test]
    fn to_openai_messages_all_roles() {
        let msgs = vec![
            LlmChatMessage {
                role: "system".into(),
                content: "sys".into(),
                tool_call_id: None,
                tool_calls: None,
                reasoning_content: None,
            },
            LlmChatMessage {
                role: "user".into(),
                content: "hi".into(),
                tool_call_id: None,
                tool_calls: None,
                reasoning_content: None,
            },
            LlmChatMessage {
                role: "assistant".into(),
                content: "hello".into(),
                tool_call_id: None,
                tool_calls: Some(vec![LlmToolCall {
                    id: "c1".into(),
                    name: "Read".into(),
                    arguments: r#"{"p":"t"}"#.into(),
                }]),
                reasoning_content: None,
            },
            LlmChatMessage {
                role: "tool".into(),
                content: "r".into(),
                tool_call_id: Some("c1".into()),
                tool_calls: None,
                reasoning_content: None,
            },
        ];
        let result = ChatClient::to_openai_messages(&msgs);
        assert_eq!(result.len(), 4);
        assert_eq!(result[0]["role"], "system");
        assert!(result[2]["tool_calls"].is_array());
    }

    #[test]
    fn sse_chunk_parses_reasoning_content() {
        let json =
            r#"{"choices":[{"delta":{"reasoning_content":"think..."},"finish_reason":null}]}"#;
        let chunk: StreamChunk = serde_json::from_str(json).unwrap();
        assert_eq!(
            chunk.choices.unwrap()[0].delta.reasoning_content.as_deref(),
            Some("think...")
        );
    }

    #[test]
    fn sse_chunk_parses_tool_call() {
        let json = r#"{"choices":[{"delta":{"tool_calls":[{"index":0,"id":"c1","function":{"name":"Read","arguments":"{}"}}]},"finish_reason":"tool_calls"}],"usage":{"prompt_tokens":100,"completion_tokens":20,"prompt_tokens_details":{"cached_tokens":50}}}"#;
        let chunk: StreamChunk = serde_json::from_str(json).unwrap();
        let usage = chunk.usage.unwrap();
        assert_eq!(usage.prompt_tokens, Some(100));
        let choices = chunk.choices.unwrap();
        assert_eq!(
            choices[0].delta.tool_calls.as_ref().unwrap()[0]
                .function
                .as_ref()
                .unwrap()
                .name
                .as_deref(),
            Some("Read")
        );
    }

    #[test]
    fn ready_tool_callback_once_per_index() {
        let mut seen: Vec<LlmToolCall> = Vec::new();
        let mut cb = |tc: LlmToolCall| seen.push(tc);

        let mut read = PendingTool {
            id: "c1".into(),
            name: Some("Read".into()),
            arguments: r#"{"path":"a.md"}"#.into(),
            start_emitted: true,
            ready_emitted: false,
        };
        let mut write = PendingTool {
            id: "c2".into(),
            name: Some("Write".into()),
            arguments: r#"{"path":"b.md","content":"x"}"#.into(),
            start_emitted: true,
            ready_emitted: false,
        };
        let mut partial = PendingTool {
            id: "c3".into(),
            name: Some("Read".into()),
            arguments: "{\"path\":\"".into(),
            start_emitted: true,
            ready_emitted: false,
        };

        ChatClient::try_emit_ready_tool(&mut read, &mut cb);
        ChatClient::try_emit_ready_tool(&mut read, &mut cb);
        ChatClient::try_emit_ready_tool(&mut write, &mut cb);
        ChatClient::try_emit_ready_tool(&mut partial, &mut cb);

        assert_eq!(seen.len(), 2);
        assert_eq!(seen[0].id, "c1");
        assert_eq!(seen[1].id, "c2");
        assert!(read.ready_emitted);
        assert!(write.ready_emitted);
        assert!(!partial.ready_emitted);
    }

    #[test]
    fn stream_accumulators_apply_tool_delta() {
        let mut acc = StreamAccumulators::new();
        let json = r#"{"choices":[{"delta":{"tool_calls":[{"index":0,"id":"c1","function":{"name":"Read","arguments":"{\"path\":\"a.md\"}"}}]}}]}"#;
        let chunk: StreamChunk = serde_json::from_str(json).unwrap();
        let mut ready: Vec<LlmToolCall> = Vec::new();
        acc.apply_chunk(
            &chunk,
            &mut |_| {},
            &mut Some(|tc: LlmToolCall| ready.push(tc)),
        );
        assert_eq!(ready.len(), 1);
        assert_eq!(ready[0].name, "Read");
    }

    #[tokio::test]
    async fn create_stream_parses_sse_via_wiremock() {
        use wiremock::matchers::{method, path};
        use wiremock::{Mock, MockServer, ResponseTemplate};

        let server = MockServer::start().await;
        let sse = "data: {\"choices\":[{\"delta\":{\"content\":\"hi\"},\"finish_reason\":null}],\"usage\":{\"prompt_tokens\":10,\"completion_tokens\":2,\"prompt_tokens_details\":{\"cached_tokens\":4}}}\n\ndata: [DONE]\n\n";
        Mock::given(method("POST"))
            .and(path("/chat/completions"))
            .respond_with(
                ResponseTemplate::new(200)
                    .insert_header("content-type", "text/event-stream")
                    .set_body_string(sse),
            )
            .mount(&server)
            .await;

        let mut client = ChatClient::deepseek("test-key", "deepseek-chat", &server.uri(), false);
        let messages = vec![LlmChatMessage {
            role: "user".into(),
            content: "ping".into(),
            tool_call_id: None,
            tool_calls: None,
            reasoning_content: None,
        }];
        let outcome = client
            .create_stream(&messages, &[], 64, |_| {}, None::<fn(LlmToolCall)>, None)
            .await
            .expect("stream ok");
        match outcome {
            StreamOutcome::Complete(c) => {
                assert_eq!(c.content.as_deref(), Some("hi"));
                assert!(c.usage.is_some());
            }
            _ => panic!("expected Complete"),
        }
    }

    #[tokio::test]
    async fn create_stream_cancel_drains_usage_via_wiremock() {
        use wiremock::matchers::{header, method, path};
        use wiremock::{Mock, MockServer, ResponseTemplate};

        let server = MockServer::start().await;
        let sse = "data: {\"choices\":[{\"delta\":{\"content\":\"x\"}}]}\n\ndata: [DONE]\n\n";
        Mock::given(method("POST"))
            .and(path("/chat/completions"))
            .and(header("accept", "text/event-stream"))
            .respond_with(
                ResponseTemplate::new(200)
                    .insert_header("content-type", "text/event-stream")
                    .set_body_string(sse),
            )
            .mount(&server)
            .await;
        Mock::given(method("POST"))
            .and(path("/chat/completions"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "usage": {
                    "prompt_tokens": 20,
                    "completion_tokens": 1,
                    "prompt_tokens_details": { "cached_tokens": 5 }
                }
            })))
            .mount(&server)
            .await;

        let mut client = ChatClient::deepseek("key", "m", &server.uri(), false);
        let messages = vec![LlmChatMessage {
            role: "user".into(),
            content: "q".into(),
            tool_call_id: None,
            tool_calls: None,
            reasoning_content: None,
        }];
        let cancel = Arc::new(AtomicBool::new(false));
        let cancel_on_event = Arc::clone(&cancel);
        let outcome = client
            .create_stream(
                &messages,
                &[],
                32,
                move |_| {
                    cancel_on_event.store(true, Ordering::SeqCst);
                },
                None::<fn(LlmToolCall)>,
                Some(cancel),
            )
            .await
            .expect("cancelled stream");
        let StreamOutcome::Cancelled {
            background_usage, ..
        } = outcome
        else {
            panic!("expected Cancelled");
        };
        let usage = tokio::time::timeout(Duration::from_secs(3), background_usage)
            .await
            .expect("drain timeout")
            .expect("drain channel")
            .expect("usage from drain");
        assert_eq!(usage.cache_hit_tokens, 5);
    }

    #[tokio::test]
    async fn web_search_parses_mock_response() {
        use wiremock::matchers::method;
        use wiremock::{Mock, MockServer, ResponseTemplate};

        let server = MockServer::start().await;
        let body = serde_json::json!({
            "content": [{
                "type": "web_search_tool_result",
                "content": [{
                    "title": "Example",
                    "url": "https://example.com",
                    "snippet": "text"
                }]
            }]
        });
        Mock::given(method("POST"))
            .respond_with(ResponseTemplate::new(200).set_body_json(body))
            .mount(&server)
            .await;

        let prev = std::env::var("DEEPSEEK_WEB_SEARCH_MESSAGES_URL").ok();
        std::env::set_var(
            "DEEPSEEK_WEB_SEARCH_MESSAGES_URL",
            format!("{}/v1/messages", server.uri()),
        );
        let results = ChatClient::web_search("key", "rust", 3)
            .await
            .expect("web_search");
        match prev {
            Some(p) => std::env::set_var("DEEPSEEK_WEB_SEARCH_MESSAGES_URL", p),
            None => std::env::remove_var("DEEPSEEK_WEB_SEARCH_MESSAGES_URL"),
        }
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].title, "Example");
    }
}
