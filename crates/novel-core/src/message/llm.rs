use super::convert::parse_tool_call_input;
use super::repair::{repair_tool_use_chain_traced, scan_tool_chain_gaps, RepairTraceContext};
use crate::{ChatMessage, ToolCallRecord};
use novel_deepseek::{LlmChatMessage, LlmCompletion, LlmToolCall};
use novel_state::StoredMessage;

pub fn to_llm_messages(messages: &[ChatMessage]) -> Vec<LlmChatMessage> {
    to_llm_messages_traced(messages, None)
}

pub fn to_llm_messages_traced(
    messages: &[ChatMessage],
    trace: Option<RepairTraceContext<'_>>,
) -> Vec<LlmChatMessage> {
    let gaps = scan_tool_chain_gaps(messages);
    if !gaps.is_empty() {
        tracing::debug!(
            gap_count = gaps.len(),
            gaps = ?gaps
                .iter()
                .map(|g| {
                    (
                        g.assistant_index,
                        g.tool_call_ids.as_slice(),
                        g.tool_names.as_slice(),
                    )
                })
                .collect::<Vec<_>>(),
            message_count = messages.len(),
            label = trace.map(|t| t.label),
            fork_run_id = trace.and_then(|t| t.fork_run_id),
            inner_turn = trace.and_then(|t| t.inner_turn),
            session_id = trace.and_then(|t| t.session_id),
            "tool_chain_gaps_before_repair"
        );
    }
    let mut repaired = messages.to_vec();
    repair_tool_use_chain_traced(&mut repaired, trace);
    repaired
        .iter()
        .map(|m| LlmChatMessage {
            role: m.role.clone(),
            content: m.content.clone(),
            tool_call_id: m.tool_call_id.clone(),
            reasoning_content: m.reasoning_content.clone(),
            tool_calls: m.tool_calls.as_ref().map(|tcs| {
                tcs.iter()
                    .map(|tc| LlmToolCall {
                        id: tc.id.clone(),
                        name: tc.name.clone(),
                        arguments: tc.arguments.to_string(),
                    })
                    .collect()
            }),
        })
        .collect()
}

pub fn stored_to_chat(stored: &[StoredMessage]) -> Result<Vec<ChatMessage>, crate::AgentError> {
    let mut out = Vec::with_capacity(stored.len());
    for s in stored {
        let content = s
            .content_json
            .get("content")
            .and_then(|v| v.as_str())
            .unwrap_or_else(|| {
                tracing::warn!(message_id = %s.id, role = %s.role, "stored message missing 'content' field in content_json");
                ""
            })
            .to_string();
        let tool_call_id = s
            .content_json
            .get("tool_call_id")
            .and_then(|v| v.as_str())
            .map(String::from);
        let reasoning_content = s
            .content_json
            .get("reasoning_content")
            .and_then(|v| v.as_str())
            .filter(|s| !s.is_empty())
            .map(String::from);
        let tool_calls = if let Some(v) = s.content_json.get("tool_calls") {
            let parsed: Vec<ToolCallRecord> = serde_json::from_value(v.clone()).map_err(|e| {
                tracing::error!(
                    message_id = %s.id,
                    role = %s.role,
                    %e,
                    "failed to deserialize tool_calls from stored message"
                );
                crate::AgentError::State(novel_state::StateError::Json(e))
            })?;
            if parsed.is_empty() {
                None
            } else {
                Some(parsed)
            }
        } else {
            None
        };
        out.push(ChatMessage {
            role: s.role.clone(),
            content,
            tool_call_id,
            tool_calls,
            reasoning_content,
        });
    }
    Ok(out)
}

pub fn assistant_from_completion(completion: &LlmCompletion) -> ChatMessage {
    assistant_with_tools(
        completion.content.clone(),
        completion.tool_calls.clone(),
        completion.reasoning_content.clone(),
    )
}

pub fn assistant_with_tools(
    content: Option<String>,
    tool_calls: Vec<LlmToolCall>,
    reasoning_content: Option<String>,
) -> ChatMessage {
    ChatMessage {
        role: "assistant".into(),
        content: content.unwrap_or_default(),
        tool_call_id: None,
        reasoning_content,
        tool_calls: if tool_calls.is_empty() {
            None
        } else {
            Some(
                tool_calls
                    .into_iter()
                    .map(|tc| ToolCallRecord {
                        id: tc.id.clone(),
                        name: tc.name.clone(),
                        arguments: parse_tool_call_input(&tc.arguments, &tc.id, &tc.name),
                    })
                    .collect(),
            )
        },
    }
}

pub fn tool_result_message(tool_call_id: &str, content: &str) -> ChatMessage {
    ChatMessage {
        role: "tool".into(),
        content: content.to_string(),
        tool_call_id: Some(tool_call_id.to_string()),
        tool_calls: None,
        reasoning_content: None,
    }
}
