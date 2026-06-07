use novel_core::Event;
use novel_tools::AskQuestion;

use super::serialize_payload;
use crate::tauri::events::ToolCallRequestPayload;

/// Frontend expects camelCase (`allowMultiple`); tool schema keeps snake_case internally.
fn ask_questions_for_ui(questions: &[AskQuestion]) -> serde_json::Value {
    serde_json::Value::Array(
        questions
            .iter()
            .map(|q| {
                serde_json::json!({
                    "id": q.id,
                    "prompt": q.prompt,
                    "options": q.options,
                    "allowMultiple": q.allow_multiple,
                    "allowCustom": q.allow_custom,
                })
            })
            .collect(),
    )
}

pub(crate) fn tool_payload(event: &Event) -> Option<(String, serde_json::Value)> {
    match event {
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
        } => serialize_payload(
            "tool-call-request",
            &ToolCallRequestPayload {
                tool_call_id: tool_call_id.clone(),
                tool_name: name.clone(),
                input: input.clone(),
                needs_approval: *needs_approval,
            },
        )
        .map(|payload| ("tool-call-request".into(), payload)),
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
        Event::AskUserQuestion {
            tool_call_id,
            payload,
        } => Some((
            "ask-user-question".into(),
            serde_json::json!({
                "toolCallId": tool_call_id,
                "questions": ask_questions_for_ui(&payload.questions),
            }),
        )),
        _ => None,
    }
}
