use crate::{ChatMessage, ToolCallRecord};
use novel_compaction::{CompactionMessage, CompactionToolCall};
use novel_deepseek::parse_tool_arguments;
use serde_json::Value;

pub fn parse_tool_call_input(raw: &str, tool_call_id: &str, tool_name: &str) -> Value {
    match parse_tool_arguments(raw) {
        Ok(v) => v,
        Err(e) => {
            tracing::warn!(
                %e,
                tool_call_id,
                tool = tool_name,
                raw,
                "failed to parse tool arguments JSON"
            );
            Value::Object(Default::default())
        }
    }
}

pub fn chat_to_compaction(msg: &ChatMessage) -> CompactionMessage {
    CompactionMessage {
        role: msg.role.clone(),
        content: msg.content.clone(),
        tool_call_id: msg.tool_call_id.clone(),
        tool_calls: msg.tool_calls.as_ref().map(|tcs| {
            tcs.iter()
                .map(|tc| CompactionToolCall {
                    id: tc.id.clone(),
                    name: tc.name.clone(),
                    arguments: tc.arguments.to_string(),
                })
                .collect()
        }),
        reasoning_content: msg.reasoning_content.clone(),
    }
}

pub fn compaction_to_chat(msg: &CompactionMessage) -> ChatMessage {
    ChatMessage {
        role: msg.role.clone(),
        content: msg.content.clone(),
        tool_call_id: msg.tool_call_id.clone(),
        reasoning_content: msg.reasoning_content.clone(),
        display_content: None,
        tool_calls: msg.tool_calls.as_ref().map(|tcs| {
            tcs.iter()
                .map(|tc| ToolCallRecord {
                    id: tc.id.clone(),
                    name: tc.name.clone(),
                    arguments: parse_tool_call_input(&tc.arguments, &tc.id, &tc.name),
                })
                .collect()
        }),
    }
}

pub fn chat_slice_to_compaction(messages: &[ChatMessage]) -> Vec<CompactionMessage> {
    messages.iter().map(chat_to_compaction).collect()
}

pub fn compaction_slice_to_chat(messages: &[CompactionMessage]) -> Vec<ChatMessage> {
    messages.iter().map(compaction_to_chat).collect()
}
