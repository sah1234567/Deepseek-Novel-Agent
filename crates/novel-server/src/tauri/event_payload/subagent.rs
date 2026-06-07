use novel_core::Event;

use super::serialize_payload;
use crate::tauri::events::SubAgentCompletePayload;

pub(super) fn subagent_payload(event: &Event) -> Option<(String, serde_json::Value)> {
    match event {
        Event::SubAgentComplete {
            fork_run_id,
            agent_id,
            output,
            ..
        } => serialize_payload(
            "sub-agent-complete",
            &SubAgentCompletePayload {
                fork_run_id: fork_run_id.clone(),
                agent_id: agent_id.clone(),
                output: output.clone(),
            },
        )
        .map(|payload| ("sub-agent-complete".into(), payload)),
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
