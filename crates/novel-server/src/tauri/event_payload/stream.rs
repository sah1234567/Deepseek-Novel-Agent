use novel_core::Event;

use super::serialize_payload;
use crate::tauri::events::{
    SessionTodosUpdatedPayload, SessionTokensUpdatedPayload, StreamChunkPayload,
    TurnCompletePayload,
};

pub(crate) fn stream_payload(
    event: &Event,
    message_id: &str,
) -> Option<(String, serde_json::Value)> {
    match event {
        Event::ContentBlockDelta { delta, kind, .. } => serialize_payload(
            "stream-chunk",
            &StreamChunkPayload {
                message_id: message_id.to_string(),
                block_index: 0,
                delta: delta.clone(),
                kind: format!("{:?}", kind).to_lowercase(),
            },
        )
        .map(|payload| ("stream-chunk".into(), payload)),
        Event::SessionTokensUpdated {
            cache_hit_tokens,
            cache_miss_tokens,
            completion_tokens,
            context_tokens,
        } => serialize_payload(
            "session-tokens-updated",
            &SessionTokensUpdatedPayload {
                cache_hit_tokens: *cache_hit_tokens,
                cache_miss_tokens: *cache_miss_tokens,
                completion_tokens: *completion_tokens,
                context_tokens: *context_tokens,
            },
        )
        .map(|payload| ("session-tokens-updated".into(), payload)),
        Event::SessionTodosUpdated { todos } => serialize_payload(
            "session-todos-updated",
            &SessionTodosUpdatedPayload {
                todos: todos.clone(),
            },
        )
        .map(|payload| ("session-todos-updated".into(), payload)),
        Event::TurnComplete {
            cache_hit_tokens,
            cache_miss_tokens,
            completion_tokens,
            turn_hit_tokens,
            turn_miss_tokens,
            turn_comp_tokens,
            was_interrupted,
            ..
        } => serialize_payload(
            "turn-complete",
            &TurnCompletePayload {
                cache_hit_tokens: *cache_hit_tokens,
                cache_miss_tokens: *cache_miss_tokens,
                completion_tokens: *completion_tokens,
                turn_hit_tokens: *turn_hit_tokens,
                turn_miss_tokens: *turn_miss_tokens,
                turn_comp_tokens: *turn_comp_tokens,
                was_interrupted: *was_interrupted,
            },
        )
        .map(|payload| ("turn-complete".into(), payload)),
        Event::TurnStart { turn_number } => Some((
            "turn-complete".into(),
            serde_json::json!({
                "phase": "start",
                "turnNumber": turn_number,
            }),
        )),
        Event::Error {
            message,
            recoverable,
        } => Some((
            "turn-complete".into(),
            serde_json::json!({
                "phase": "error",
                "message": message,
                "recoverable": recoverable,
            }),
        )),
        Event::AssistantSegmentComplete {
            segment_index,
            fork_run_id,
        } => Some((
            "assistant-segment-complete".into(),
            serde_json::json!({
                "segmentIndex": segment_index,
                "forkRunId": fork_run_id,
            }),
        )),
        _ => None,
    }
}
