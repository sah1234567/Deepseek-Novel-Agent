use serde::Serialize;

#[derive(Debug, Clone)]
pub struct LlmChatMessage {
    pub role: String,
    pub content: String,
    pub tool_call_id: Option<String>,
    pub tool_calls: Option<Vec<LlmToolCall>>,
    /// Preserved for DeepSeek V3.2+: must be included on assistant messages
    /// when sending back to the API in multi-turn conversations.
    pub reasoning_content: Option<String>,
}

#[derive(Debug, Clone)]
pub struct LlmToolCall {
    pub id: String,
    pub name: String,
    pub arguments: String,
}

#[derive(Debug, Clone, Default)]
pub struct LlmCompletion {
    pub content: Option<String>,
    pub tool_calls: Vec<LlmToolCall>,
    pub usage: Option<TokenUsage>,
    /// Reasoning/CoT content from the assistant. Must be preserved and
    /// sent back in multi-turn conversations for DeepSeek V3.2+.
    pub reasoning_content: Option<String>,
    pub stop_reason: Option<String>,
}

#[derive(Debug, Clone, Default)]
pub struct TokenUsage {
    pub cache_hit_tokens: i64,
    pub cache_miss_tokens: i64,
    pub completion_tokens: i64,
    /// Thinking/reasoning tokens within completion_tokens (0 if no CoT).
    pub reasoning_tokens: i64,
}

impl TokenUsage {
    pub fn from_deepseek_usage(hit: i64, miss: i64, completion: i64, reasoning: i64) -> Self {
        Self {
            cache_hit_tokens: hit,
            cache_miss_tokens: miss,
            completion_tokens: completion,
            reasoning_tokens: reasoning,
        }
    }

    pub fn total_prompt(&self) -> i64 {
        self.cache_hit_tokens + self.cache_miss_tokens
    }

    pub fn cache_hit_rate(&self) -> f64 {
        let denom = self.total_prompt();
        if denom == 0 {
            0.0
        } else {
            self.cache_hit_tokens as f64 / denom as f64
        }
    }
}

/// Receiver for token usage from a background stream drain.
pub type BackgroundUsageRx = tokio::sync::oneshot::Receiver<Option<TokenUsage>>;

#[derive(Debug)]
pub enum StreamOutcome {
    Complete(LlmCompletion),
    Cancelled {
        partial: LlmCompletion,
        background_usage: BackgroundUsageRx,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum StreamEvent {
    ContentBlockDelta {
        index: u32,
        delta: String,
        kind: ContentBlockKind,
    },
    ToolUseStarted {
        index: u32,
        tool_call_id: String,
        name: String,
    },
    ToolInputDelta {
        tool_call_id: String,
        delta: String,
    },
    MessageStop {
        stop_reason: Option<String>,
    },
    StreamError {
        message: String,
        retryable: bool,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub enum ContentBlockKind {
    Text,
    Thinking,
    ToolCall,
}

/// Per-request overrides for [`crate::ChatClient::create_stream`].
///
/// Unset fields fall back to the client's defaults (model, thinking_enabled).
/// This keeps the client as the long-lived profile while allowing per-call
/// adjustments (e.g. memory side-query force-disables thinking and passes
/// a `response_format` schema).
#[derive(Debug, Clone, Default)]
pub struct ChatRequestOptions {
    /// `None` → use `ChatClient::thinking_enabled`.
    pub thinking: Option<bool>,
    /// Structured output schema (e.g. memory selector JSON schema).
    /// Main turn / subagent / compaction do not use this.
    pub response_format: Option<serde_json::Value>,
}

impl ChatRequestOptions {
    /// Convenience constructor for memory side-queries: always disables
    /// thinking (defensive — even if the flash client was misconfigured)
    /// and attaches the JSON schema for structured output.
    pub fn for_memory_side_query(response_format: Option<serde_json::Value>) -> Self {
        Self {
            thinking: Some(false),
            response_format,
        }
    }

    /// Resolve the effective thinking flag for this request.
    pub(crate) fn resolve_thinking(&self, client: &crate::ChatClient) -> bool {
        self.thinking.unwrap_or(client.thinking_enabled)
    }
}

/// A single web search result from DeepSeek's `web_search_20250305` server-side tool.
#[derive(Debug, Clone, Serialize)]
pub struct WebSearchResult {
    pub title: String,
    pub url: String,
    pub snippet: String,
}
