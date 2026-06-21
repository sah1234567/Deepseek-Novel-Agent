//! Memory selection: `MemorySelector` trait (DIP), `select_relevant` pipeline,
//! and the `ChatClient` implementation.
//!
//! Types live in [`crate::memory_types`]; prompt constants in [`crate::memory_select`].

use crate::memory_scan::format_memory_manifest;
use crate::memory_select::{
    parse_selector_response, SELECT_MEMORIES_SCHEMA, SELECT_MEMORIES_SYSTEM_PROMPT,
};
use crate::memory_types::{MemoryConstants, MemoryHeader, SideQueryResult};
use async_trait::async_trait;
use novel_deepseek::{
    ChatClient, ChatRequestOptions, LlmChatMessage, LlmError, LlmToolCall, StreamOutcome,
};

/// Abstraction for selecting relevant memories via a fast LLM call.
///
/// Conforms to DIP (engineering-principles §7): high-level modules depend on
/// this trait, not on the concrete `ChatClient`. Tests can inject a mock
/// implementation without a real API key.
///
/// Uses `&mut self` because the default implementation (`ChatClient`) writes
/// its cache tracker during the streaming request.
#[async_trait]
pub trait MemorySelector: Send + Sync {
    async fn side_query(
        &mut self,
        system: &str,
        user_message: &str,
        max_tokens: u32,
        response_format: Option<serde_json::Value>,
    ) -> Result<SideQueryResult, LlmError>;
}

// ── ChatClient impl: unified streaming path via create_stream ──

#[async_trait]
impl MemorySelector for ChatClient {
    async fn side_query(
        &mut self,
        system: &str,
        user_message: &str,
        max_tokens: u32,
        response_format: Option<serde_json::Value>,
    ) -> Result<SideQueryResult, LlmError> {
        let messages = vec![
            LlmChatMessage {
                role: "system".into(),
                content: system.into(),
                tool_call_id: None,
                tool_calls: None,
                reasoning_content: None,
            },
            LlmChatMessage {
                role: "user".into(),
                content: user_message.into(),
                tool_call_id: None,
                tool_calls: None,
                reasoning_content: None,
            },
        ];
        let options = ChatRequestOptions::for_memory_side_query(response_format);
        let outcome = self
            .create_stream(
                &messages,
                &[],
                max_tokens,
                options,
                |_| {},
                None::<fn(LlmToolCall)>,
                None,
            )
            .await?;
        let completion = match outcome {
            StreamOutcome::Complete(c) => c,
            StreamOutcome::Cancelled { .. } => return Err(LlmError::Cancelled),
        };
        Ok(SideQueryResult {
            content: completion.content.unwrap_or_default(),
        })
    }
}

/// Select the most relevant memory files (up to 5) for the current writing task.
///
/// Sends the memory manifest to a fast model via `side_query`, then parses the
/// JSON response to extract selected filenames. Validates that returned
/// filenames actually exist in the candidates list.
pub async fn select_relevant(
    selector: &mut impl MemorySelector,
    query: &str,
    memories: &[MemoryHeader],
) -> Result<Vec<String>, LlmError> {
    if memories.is_empty() {
        return Ok(Vec::new());
    }

    let manifest = format_memory_manifest(memories, false);
    let user_message = format!("Task: {query}\n\nAvailable memories:\n{manifest}");

    let schema: serde_json::Value =
        serde_json::from_str(SELECT_MEMORIES_SCHEMA).unwrap_or_default();

    let result: SideQueryResult = selector
        .side_query(
            SELECT_MEMORIES_SYSTEM_PROMPT,
            &user_message,
            MemoryConstants::FLASH_MAX_TOKENS,
            Some(schema),
        )
        .await?;

    let selected = parse_selector_response(&result.content);

    // Validate that selected filenames actually exist in the manifest
    let valid: Vec<String> = selected
        .into_iter()
        .filter(|name| memories.iter().any(|h| h.rel_path == *name))
        .take(5)
        .collect();

    Ok(valid)
}

/// Create a memory selector from the global API config file, falling back
/// to `DEEPSEEK_API_KEY` env var. Returns `None` if no API key is available
/// (offline/test mode).
///
/// Uses [`novel_config::ModelConfig::memory_selector`] for the canonical
/// flash model + thinking settings.
pub fn create_selector_from_config(global_config_path: &std::path::Path) -> Option<ChatClient> {
    let cfg = novel_config::ModelConfig::memory_selector();
    let api_key = novel_config::resolve_agent_api_key(global_config_path);
    let api_base = novel_config::resolve_agent_api_base(global_config_path);
    ChatClient::from_api_key_or_env(
        api_key.as_deref(),
        &api_base,
        &cfg.model,
        cfg.thinking_enabled,
    )
}
