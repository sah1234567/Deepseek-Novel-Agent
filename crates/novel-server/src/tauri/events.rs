use novel_core::Event;
use serde::Serialize;
use tauri::{AppHandle, Emitter};

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
    match event {
        Event::ContentBlockDelta { delta, kind, .. } => {
            let _ = app.emit(
                "stream-chunk",
                StreamChunkPayload {
                    message_id: message_id.to_string(),
                    block_index: 0,
                    delta,
                    kind: format!("{:?}", kind).to_lowercase(),
                },
            );
        }
        Event::ToolUseStarted { tool_call_id, name } => {
            let _ = app.emit(
                "tool-call-request",
                serde_json::json!({
                    "phase": "start",
                    "toolCallId": tool_call_id,
                    "toolName": name,
                }),
            );
        }
        Event::ToolInputDelta {
            tool_call_id,
            delta,
        } => {
            let _ = app.emit(
                "tool-call-request",
                serde_json::json!({
                    "phase": "input_delta",
                    "toolCallId": tool_call_id,
                    "delta": delta,
                }),
            );
        }
        Event::ToolInputComplete {
            tool_call_id,
            name,
            input,
            needs_approval,
        } => {
            let _ = app.emit(
                "tool-call-request",
                serde_json::json!({
                    "phase": "input_complete",
                    "toolCallId": tool_call_id,
                    "toolName": name,
                    "input": input,
                    "needsApproval": needs_approval,
                }),
            );
        }
        Event::ToolCallRequest {
            tool_call_id,
            name,
            input,
            needs_approval,
        } => {
            let _ = app.emit(
                "tool-call-request",
                ToolCallRequestPayload {
                    tool_call_id,
                    tool_name: name,
                    input,
                    needs_approval,
                },
            );
        }
        Event::ToolCallProgress {
            tool_call_id,
            status,
            description,
        } => {
            let _ = app.emit(
                "tool-call-request",
                serde_json::json!({
                    "phase": "progress",
                    "toolCallId": tool_call_id,
                    "status": status,
                    "description": description,
                }),
            );
        }
        Event::TurnComplete {
            cache_hit_tokens,
            cache_miss_tokens,
            completion_tokens,
            turn_hit_tokens,
            turn_miss_tokens,
            turn_comp_tokens,
            was_interrupted,
            ..
        } => {
            let _ = app.emit(
                "turn-complete",
                TurnCompletePayload {
                    cache_hit_tokens,
                    cache_miss_tokens,
                    completion_tokens,
                    turn_hit_tokens,
                    turn_miss_tokens,
                    turn_comp_tokens,
                    was_interrupted,
                },
            );
        }
        Event::SubAgentComplete {
            fork_run_id,
            agent_id,
            output,
            ..
        } => {
            let _ = app.emit(
                "sub-agent-complete",
                SubAgentCompletePayload {
                    fork_run_id,
                    agent_id,
                    output,
                },
            );
        }
        Event::SubAgentStarted {
            fork_run_id,
            agent_type,
            task_preview,
            ..
        } => {
            let _ = app.emit(
                "sub-agent-started",
                serde_json::json!({
                    "forkRunId": fork_run_id,
                    "agentType": agent_type,
                    "taskPreview": task_preview,
                }),
            );
        }
        Event::SubAgentStreamDelta {
            fork_run_id,
            delta,
            kind,
        } => {
            let _ = app.emit(
                "sub-agent-stream",
                serde_json::json!({
                    "forkRunId": fork_run_id,
                    "delta": delta,
                    "kind": format!("{:?}", kind).to_lowercase(),
                }),
            );
        }
        Event::SubAgentToolUpdate {
            fork_run_id,
            phase,
            tool_call_id,
            tool_name,
            input,
            content,
            needs_approval,
            status,
            description,
        } => {
            let _ = app.emit(
                "sub-agent-tool",
                serde_json::json!({
                    "forkRunId": fork_run_id,
                    "phase": phase,
                    "toolCallId": tool_call_id,
                    "toolName": tool_name,
                    "input": input,
                    "content": content,
                    "needsApproval": needs_approval,
                    "status": status,
                    "description": description,
                }),
            );
        }
        Event::ToolCallResult {
            tool_call_id,
            content,
        } => {
            let _ = app.emit(
                "tool-call-request",
                serde_json::json!({
                    "phase": "result",
                    "toolCallId": tool_call_id,
                    "content": content,
                }),
            );
        }
        Event::TurnStart { turn_number } => {
            let _ = app.emit(
                "turn-complete",
                serde_json::json!({
                    "phase": "start",
                    "turnNumber": turn_number,
                }),
            );
        }
        Event::AskUserQuestion {
            tool_call_id,
            payload,
        } => {
            let _ = app.emit(
                "ask-user-question",
                serde_json::json!({
                    "toolCallId": tool_call_id,
                    "questions": payload.questions,
                }),
            );
        }
        Event::CompactionProgress { attempt, action } => {
            let payload = match action {
                novel_core::CompactionAction::Started => {
                    serde_json::json!({ "attempt": attempt, "action": "started" })
                }
                novel_core::CompactionAction::GeneratingSummary => {
                    serde_json::json!({ "attempt": attempt, "action": "generating-summary" })
                }
                novel_core::CompactionAction::RebuildingSession => {
                    serde_json::json!({ "attempt": attempt, "action": "rebuilding-session" })
                }
                novel_core::CompactionAction::Done { tokens_before, tokens_after } => {
                    serde_json::json!({
                        "attempt": attempt,
                        "action": "done",
                        "tokensBefore": tokens_before,
                        "tokensAfter": tokens_after,
                    })
                }
                novel_core::CompactionAction::Failed { reason } => {
                    serde_json::json!({
                        "attempt": attempt,
                        "action": "failed",
                        "reason": reason,
                    })
                }
            };
            let _ = app.emit("compaction-progress", payload);
        }
        Event::Error {
            message,
            recoverable,
        } => {
            let _ = app.emit(
                "turn-complete",
                serde_json::json!({
                    "phase": "error",
                    "message": message,
                    "recoverable": recoverable,
                }),
            );
        }
        Event::AssistantSegmentComplete {
            segment_index,
            fork_run_id,
        } => {
            let _ = app.emit(
                "assistant-segment-complete",
                serde_json::json!({
                    "segmentIndex": segment_index,
                    "forkRunId": fork_run_id,
                }),
            );
        }
    }
}
