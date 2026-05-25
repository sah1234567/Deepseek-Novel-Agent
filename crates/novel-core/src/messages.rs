use crate::interrupt::{INTERRUPT_MESSAGE, INTERRUPT_MESSAGE_FOR_TOOL_USE};
use crate::ChatMessage;

pub fn create_user_interruption_message(tool_use: bool) -> ChatMessage {
    ChatMessage {
        role: "user".into(),
        content: if tool_use {
            INTERRUPT_MESSAGE_FOR_TOOL_USE.into()
        } else {
            INTERRUPT_MESSAGE.into()
        },
        tool_call_id: None,
        tool_calls: None,
        reasoning_content: None,
    }
}

/// Synthetic tool_result messages for tool_use blocks missing results after abort.
pub fn yield_missing_tool_result_blocks(
    assistant: &ChatMessage,
    error_message: &str,
) -> Vec<ChatMessage> {
    let Some(tool_calls) = &assistant.tool_calls else {
        return Vec::new();
    };
    tool_calls
        .iter()
        .map(|tc| ChatMessage {
            role: "tool".into(),
            content: error_message.to_string(),
            tool_call_id: Some(tc.id.clone()),
            tool_calls: None,
            reasoning_content: None,
        })
        .collect()
}

pub fn is_synthetic_message(msg: &ChatMessage) -> bool {
    msg.role == "user"
        && (msg.content == INTERRUPT_MESSAGE || msg.content == INTERRUPT_MESSAGE_FOR_TOOL_USE)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ToolCallRecord;
    use serde_json::json;

    #[test]
    fn interruption_message_variants() {
        let plain = create_user_interruption_message(false);
        assert_eq!(plain.content, INTERRUPT_MESSAGE);
        let tool = create_user_interruption_message(true);
        assert_eq!(tool.content, INTERRUPT_MESSAGE_FOR_TOOL_USE);
    }

    #[test]
    fn missing_tool_results_from_assistant() {
        let assistant = ChatMessage {
            role: "assistant".into(),
            content: String::new(),
            tool_call_id: None,
            reasoning_content: None,
            tool_calls: Some(vec![ToolCallRecord {
                id: "t1".into(),
                name: "Read".into(),
                arguments: json!({}),
            }]),
        };
        let blocks = yield_missing_tool_result_blocks(&assistant, "Interrupted by user");
        assert_eq!(blocks.len(), 1);
        assert_eq!(blocks[0].role, "tool");
        assert_eq!(blocks[0].tool_call_id.as_deref(), Some("t1"));
    }

    #[test]
    fn is_synthetic_detects_interrupt_messages() {
        let plain = create_user_interruption_message(false);
        assert!(is_synthetic_message(&plain));
        let normal = ChatMessage {
            role: "user".into(),
            content: "hello".into(),
            tool_call_id: None,
            tool_calls: None,
            reasoning_content: None,
        };
        assert!(!is_synthetic_message(&normal));
    }
}
