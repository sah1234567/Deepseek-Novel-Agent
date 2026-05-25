mod cache;
mod client;
mod config;
mod connectivity;
mod error;
mod tool_args;
mod types;

pub use cache::{CacheStats, CacheTracker};
pub use client::ChatClient;
pub use config::{chat_api_base, defaults, web_search_messages_url, DeepSeekEndpoints};
pub use connectivity::{verify_chat_endpoint, verify_endpoints, verify_web_search_endpoint};
pub use error::{is_context_length_exceeded, is_output_truncated, LlmError};
pub use tool_args::{parse_tool_arguments, ToolParseError};
pub use types::{
    BackgroundUsageRx, ContentBlockKind, LlmChatMessage, LlmCompletion, LlmToolCall,
    StreamEvent, StreamOutcome, TokenUsage, WebSearchResult,
};
