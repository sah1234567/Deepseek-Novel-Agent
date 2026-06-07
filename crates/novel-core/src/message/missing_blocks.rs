use crate::ChatMessage;

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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ToolCallRecord;
    use serde_json::json;

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
}
