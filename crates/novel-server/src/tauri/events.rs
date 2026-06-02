use novel_core::Event;
use serde::Serialize;
use tauri::{AppHandle, Emitter};

use super::event_payload;

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct StreamChunkPayload {
    pub message_id: String,
    pub block_index: u32,
    pub delta: String,
    pub kind: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ToolCallRequestPayload {
    pub tool_call_id: String,
    pub tool_name: String,
    pub input: serde_json::Value,
    pub needs_approval: bool,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SessionTokensUpdatedPayload {
    pub cache_hit_tokens: i64,
    pub cache_miss_tokens: i64,
    pub completion_tokens: i64,
    pub context_tokens: i64,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TurnCompletePayload {
    pub cache_hit_tokens: i64,
    pub cache_miss_tokens: i64,
    pub completion_tokens: i64,
    pub turn_hit_tokens: i64,
    pub turn_miss_tokens: i64,
    pub turn_comp_tokens: i64,
    pub was_interrupted: bool,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SubAgentCompletePayload {
    pub fork_run_id: String,
    pub agent_id: String,
    pub output: String,
}

pub fn emit_core_event(app: &AppHandle, event: Event, message_id: &str) {
    if let Some((name, payload)) = event_payload::core_event_payload(&event, message_id) {
        let _ = app.emit(&name, payload);
    }
}
