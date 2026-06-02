//! Map core engine events to Tauri emit `(event_name, payload)` pairs.

use novel_core::{CompactionAction, Event};

use super::events::{
    SessionTokensUpdatedPayload, StreamChunkPayload, SubAgentCompletePayload,
    ToolCallRequestPayload, TurnCompletePayload,
};

fn compaction_progress_payload(attempt: u32, action: &CompactionAction) -> serde_json::Value {
    match action {
        CompactionAction::Started => {
            serde_json::json!({ "attempt": attempt, "action": "started" })
        }
        CompactionAction::GeneratingSummary => {
            serde_json::json!({ "attempt": attempt, "action": "generating-summary" })
        }
        CompactionAction::RebuildingSession => {
            serde_json::json!({ "attempt": attempt, "action": "rebuilding-session" })
        }
        CompactionAction::Done {
            tokens_before,
            tokens_after,
        } => serde_json::json!({
            "attempt": attempt,
            "action": "done",
            "tokensBefore": tokens_before,
            "tokensAfter": tokens_after,
        }),
        CompactionAction::Failed { reason } => serde_json::json!({
            "attempt": attempt,
            "action": "failed",
            "reason": reason,
        }),
    }
}

fn subagent_payload(event: &Event) -> Option<(String, serde_json::Value)> {
    match event {
        Event::SubAgentComplete {
            fork_run_id,
            agent_id,
            output,
            ..
        } => Some((
            "sub-agent-complete".into(),
            serde_json::to_value(SubAgentCompletePayload {
                fork_run_id: fork_run_id.clone(),
                agent_id: agent_id.clone(),
                output: output.clone(),
            })
            .ok()?,
        )),
        Event::SubAgentStarted {
            fork_run_id,
            agent_type,
            task_preview,
            parent_tool_call_id,
            ..
        } => Some((
            "sub-agent-started".into(),
            serde_json::json!({
                "forkRunId": fork_run_id,
                "agentType": agent_type,
                "taskPreview": task_preview,
                "parentToolCallId": parent_tool_call_id,
            }),
        )),
        Event::SubAgentStreamDelta {
            fork_run_id,
            delta,
            kind,
        } => Some((
            "sub-agent-stream".into(),
            serde_json::json!({
                "forkRunId": fork_run_id,
                "delta": delta,
                "kind": format!("{:?}", kind).to_lowercase(),
            }),
        )),
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
        } => Some((
            "sub-agent-tool".into(),
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
        )),
        _ => None,
    }
}

/// Returns `(tauri_event_name, json_payload)` when the event should be forwarded to the UI.
pub(crate) fn core_event_payload(
    event: &Event,
    message_id: &str,
) -> Option<(String, serde_json::Value)> {
    if let Some(pair) = subagent_payload(event) {
        return Some(pair);
    }
    match event {
        Event::ContentBlockDelta { delta, kind, .. } => Some((
            "stream-chunk".into(),
            serde_json::to_value(StreamChunkPayload {
                message_id: message_id.to_string(),
                block_index: 0,
                delta: delta.clone(),
                kind: format!("{:?}", kind).to_lowercase(),
            })
            .ok()?,
        )),
        Event::ToolUseStarted { tool_call_id, name } => Some((
            "tool-call-request".into(),
            serde_json::json!({
                "phase": "start",
                "toolCallId": tool_call_id,
                "toolName": name,
            }),
        )),
        Event::ToolInputDelta {
            tool_call_id,
            delta,
        } => Some((
            "tool-call-request".into(),
            serde_json::json!({
                "phase": "input_delta",
                "toolCallId": tool_call_id,
                "delta": delta,
            }),
        )),
        Event::ToolInputComplete {
            tool_call_id,
            name,
            input,
            needs_approval,
        } => Some((
            "tool-call-request".into(),
            serde_json::json!({
                "phase": "input_complete",
                "toolCallId": tool_call_id,
                "toolName": name,
                "input": input,
                "needsApproval": needs_approval,
            }),
        )),
        Event::ToolCallRequest {
            tool_call_id,
            name,
            input,
            needs_approval,
        } => Some((
            "tool-call-request".into(),
            serde_json::to_value(ToolCallRequestPayload {
                tool_call_id: tool_call_id.clone(),
                tool_name: name.clone(),
                input: input.clone(),
                needs_approval: *needs_approval,
            })
            .ok()?,
        )),
        Event::ToolCallProgress {
            tool_call_id,
            status,
            description,
        } => Some((
            "tool-call-request".into(),
            serde_json::json!({
                "phase": "progress",
                "toolCallId": tool_call_id,
                "status": status,
                "description": description,
            }),
        )),
        Event::SessionTokensUpdated {
            cache_hit_tokens,
            cache_miss_tokens,
            completion_tokens,
            context_tokens,
        } => Some((
            "session-tokens-updated".into(),
            serde_json::to_value(SessionTokensUpdatedPayload {
                cache_hit_tokens: *cache_hit_tokens,
                cache_miss_tokens: *cache_miss_tokens,
                completion_tokens: *completion_tokens,
                context_tokens: *context_tokens,
            })
            .ok()?,
        )),
        Event::TurnComplete {
            cache_hit_tokens,
            cache_miss_tokens,
            completion_tokens,
            turn_hit_tokens,
            turn_miss_tokens,
            turn_comp_tokens,
            was_interrupted,
            ..
        } => Some((
            "turn-complete".into(),
            serde_json::to_value(TurnCompletePayload {
                cache_hit_tokens: *cache_hit_tokens,
                cache_miss_tokens: *cache_miss_tokens,
                completion_tokens: *completion_tokens,
                turn_hit_tokens: *turn_hit_tokens,
                turn_miss_tokens: *turn_miss_tokens,
                turn_comp_tokens: *turn_comp_tokens,
                was_interrupted: *was_interrupted,
            })
            .ok()?,
        )),
        Event::ToolCallResult {
            tool_call_id,
            content,
        } => Some((
            "tool-call-request".into(),
            serde_json::json!({
                "phase": "result",
                "toolCallId": tool_call_id,
                "content": content,
            }),
        )),
        Event::TurnStart { turn_number } => Some((
            "turn-complete".into(),
            serde_json::json!({
                "phase": "start",
                "turnNumber": turn_number,
            }),
        )),
        Event::AskUserQuestion {
            tool_call_id,
            payload,
        } => Some((
            "ask-user-question".into(),
            serde_json::json!({
                "toolCallId": tool_call_id,
                "questions": payload.questions,
            }),
        )),
        Event::CompactionProgress { attempt, action } => Some((
            "compaction-progress".into(),
            compaction_progress_payload(*attempt, action),
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

#[cfg(test)]
mod tests {
    use super::*;
    use novel_core::ContentBlockKind;

    #[test]
    fn stream_chunk_payload() {
        let event = Event::ContentBlockDelta {
            message_id: "ignored".into(),
            index: 1,
            delta: "hi".into(),
            kind: ContentBlockKind::Text,
        };
        let (name, payload) = core_event_payload(&event, "msg-1").unwrap();
        assert_eq!(name, "stream-chunk");
        assert_eq!(payload["messageId"], "msg-1");
        assert_eq!(payload["delta"], "hi");
        assert_eq!(payload["kind"], "text");
    }

    #[test]
    fn tool_call_request_payload() {
        let event = Event::ToolCallRequest {
            tool_call_id: "tc1".into(),
            name: "Read".into(),
            input: serde_json::json!({ "path": "a.md" }),
            needs_approval: true,
        };
        let (name, payload) = core_event_payload(&event, "msg-1").unwrap();
        assert_eq!(name, "tool-call-request");
        assert_eq!(payload["toolCallId"], "tc1");
        assert_eq!(payload["toolName"], "Read");
        assert_eq!(payload["needsApproval"], true);
    }

    #[test]
    fn compaction_done_payload() {
        let event = Event::CompactionProgress {
            attempt: 2,
            action: CompactionAction::Done {
                tokens_before: 100,
                tokens_after: 40,
            },
        };
        let (name, payload) = core_event_payload(&event, "msg-1").unwrap();
        assert_eq!(name, "compaction-progress");
        assert_eq!(payload["action"], "done");
        assert_eq!(payload["tokensBefore"], 100);
        assert_eq!(payload["tokensAfter"], 40);
    }

    #[test]
    fn compaction_started_payload() {
        let event = Event::CompactionProgress {
            attempt: 1,
            action: CompactionAction::Started,
        };
        let (name, payload) = core_event_payload(&event, "m").unwrap();
        assert_eq!(name, "compaction-progress");
        assert_eq!(payload["action"], "started");
    }

    #[test]
    fn turn_complete_payload() {
        let event = Event::TurnComplete {
            cache_hit_tokens: 1,
            cache_miss_tokens: 2,
            completion_tokens: 3,
            turn_hit_tokens: 4,
            turn_miss_tokens: 5,
            turn_comp_tokens: 6,
            was_interrupted: false,
            turn_number: 1,
        };
        let (name, _) = core_event_payload(&event, "m").unwrap();
        assert_eq!(name, "turn-complete");
    }

    #[test]
    fn subagent_complete_payload() {
        let event = Event::SubAgentComplete {
            fork_run_id: "f1".into(),
            agent_id: "a1".into(),
            output: "done".into(),
            cache_hit_rate: 0.5,
        };
        let (name, payload) = core_event_payload(&event, "m").unwrap();
        assert_eq!(name, "sub-agent-complete");
        assert_eq!(payload["output"], "done");
    }

    #[test]
    fn subagent_started_payload() {
        let event = Event::SubAgentStarted {
            fork_run_id: "f1".into(),
            agent_id: "a1".into(),
            agent_type: "general".into(),
            task_preview: "preview".into(),
            parent_tool_call_id: Some("tc-parent".into()),
        };
        let (name, payload) = core_event_payload(&event, "m").unwrap();
        assert_eq!(name, "sub-agent-started");
        assert_eq!(payload["forkRunId"], "f1");
        assert_eq!(payload["parentToolCallId"], "tc-parent");
    }

    #[test]
    fn subagent_stream_delta_payload() {
        let event = Event::SubAgentStreamDelta {
            fork_run_id: "f1".into(),
            delta: "chunk".into(),
            kind: ContentBlockKind::Thinking,
        };
        let (name, payload) = core_event_payload(&event, "m").unwrap();
        assert_eq!(name, "sub-agent-stream");
        assert_eq!(payload["delta"], "chunk");
        assert_eq!(payload["kind"], "thinking");
    }

    #[test]
    fn subagent_tool_update_payload() {
        let event = Event::SubAgentToolUpdate {
            fork_run_id: "f1".into(),
            phase: "start".into(),
            tool_call_id: "tc1".into(),
            tool_name: Some("Read".into()),
            input: Some(serde_json::json!({})),
            content: None,
            needs_approval: Some(true),
            status: Some("running".into()),
            description: Some("desc".into()),
        };
        let (name, payload) = core_event_payload(&event, "m").unwrap();
        assert_eq!(name, "sub-agent-tool");
        assert_eq!(payload["toolName"], "Read");
        assert_eq!(payload["needsApproval"], true);
    }

    #[test]
    fn tool_use_started_payload() {
        let event = Event::ToolUseStarted {
            tool_call_id: "tc1".into(),
            name: "Write".into(),
        };
        let (name, payload) = core_event_payload(&event, "m").unwrap();
        assert_eq!(name, "tool-call-request");
        assert_eq!(payload["phase"], "start");
        assert_eq!(payload["toolName"], "Write");
    }

    #[test]
    fn tool_input_delta_and_complete_payload() {
        let delta = Event::ToolInputDelta {
            tool_call_id: "tc1".into(),
            delta: "{".into(),
        };
        let (n1, p1) = core_event_payload(&delta, "m").unwrap();
        assert_eq!(n1, "tool-call-request");
        assert_eq!(p1["phase"], "input_delta");

        let complete = Event::ToolInputComplete {
            tool_call_id: "tc1".into(),
            name: "Read".into(),
            input: serde_json::json!({ "path": "x" }),
            needs_approval: false,
        };
        let (n2, p2) = core_event_payload(&complete, "m").unwrap();
        assert_eq!(n2, "tool-call-request");
        assert_eq!(p2["phase"], "input_complete");
    }

    #[test]
    fn tool_progress_result_and_session_tokens() {
        let progress = Event::ToolCallProgress {
            tool_call_id: "tc1".into(),
            status: "ok".into(),
            description: "done".into(),
        };
        let (n, p) = core_event_payload(&progress, "m").unwrap();
        assert_eq!(n, "tool-call-request");
        assert_eq!(p["phase"], "progress");

        let result = Event::ToolCallResult {
            tool_call_id: "tc1".into(),
            content: "body".into(),
        };
        let (_, pr) = core_event_payload(&result, "m").unwrap();
        assert_eq!(pr["phase"], "result");

        let tokens = Event::SessionTokensUpdated {
            cache_hit_tokens: 1,
            cache_miss_tokens: 2,
            completion_tokens: 3,
            context_tokens: 4,
        };
        let (tn, _) = core_event_payload(&tokens, "m").unwrap();
        assert_eq!(tn, "session-tokens-updated");
    }

    #[test]
    fn turn_start_error_and_assistant_segment() {
        let start = Event::TurnStart { turn_number: 3 };
        let (n, p) = core_event_payload(&start, "m").unwrap();
        assert_eq!(n, "turn-complete");
        assert_eq!(p["phase"], "start");

        let err = Event::Error {
            message: "boom".into(),
            recoverable: true,
        };
        let (_, pe) = core_event_payload(&err, "m").unwrap();
        assert_eq!(pe["phase"], "error");

        let seg = Event::AssistantSegmentComplete {
            segment_index: 2,
            fork_run_id: Some("f1".into()),
        };
        let (sn, _) = core_event_payload(&seg, "m").unwrap();
        assert_eq!(sn, "assistant-segment-complete");
    }

    #[test]
    fn ask_user_question_payload() {
        use novel_tools::AskUserQuestionPayload;
        let payload: AskUserQuestionPayload = serde_json::from_value(serde_json::json!({
            "questions": [{
                "id": "q1",
                "prompt": "Pick?",
                "options": [{ "id": "a", "label": "A" }]
            }]
        }))
        .unwrap();
        let event = Event::AskUserQuestion {
            tool_call_id: "tc1".into(),
            payload,
        };
        let (name, payload) = core_event_payload(&event, "m").unwrap();
        assert_eq!(name, "ask-user-question");
        assert!(payload["questions"].is_array());
    }

    #[test]
    fn compaction_all_actions() {
        for (action, expected) in [
            (CompactionAction::GeneratingSummary, "generating-summary"),
            (CompactionAction::RebuildingSession, "rebuilding-session"),
            (
                CompactionAction::Failed {
                    reason: "oops".into(),
                },
                "failed",
            ),
        ] {
            let event = Event::CompactionProgress { attempt: 1, action };
            let (_, payload) = core_event_payload(&event, "m").unwrap();
            assert_eq!(payload["action"], expected);
        }
    }

    #[test]
    fn turn_start_not_routed_via_subagent_helper() {
        // TurnStart is handled in the main match, not subagent_payload.
        let (name, _) = core_event_payload(&Event::TurnStart { turn_number: 1 }, "m").unwrap();
        assert_eq!(name, "turn-complete");
    }
}
