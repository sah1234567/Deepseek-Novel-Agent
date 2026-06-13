use novel_core::Event;
use serde::Serialize;
use std::sync::atomic::{AtomicU64, Ordering};
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
pub struct SessionTodosUpdatedPayload {
    pub todos: Vec<novel_state::SessionTodo>,
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

static LAST_EMIT_FAIL_MS: AtomicU64 = AtomicU64::new(0);

fn now_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

pub fn emit_core_event(app: &AppHandle, event: Event, message_id: &str) {
    if let Some((name, payload)) = event_payload::core_event_payload(&event, message_id) {
        if let Err(e) = app.emit(&name, payload) {
            let now = now_ms();
            let last = LAST_EMIT_FAIL_MS.load(Ordering::Relaxed);
            if now.saturating_sub(last) >= 1000 {
                LAST_EMIT_FAIL_MS.store(now, Ordering::Relaxed);
                tracing::warn!(event = %name, error = %e, "tauri emit failed");
            }
        }
    }
}
