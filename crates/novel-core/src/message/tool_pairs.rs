//! Shared assistant `tool_calls` ↔ `tool` result pairing for repair and read-cache rebuild.

use crate::{ChatMessage, ToolCallRecord};
use std::collections::HashMap;

pub(crate) struct ToolUseResultPair<'a> {
    pub call: &'a ToolCallRecord,
    pub result: &'a ChatMessage,
}

/// Collect tool_use ↔ tool_result pairs in transcript order starting at `from`.
pub(crate) fn collect_tool_use_result_pairs<'a>(
    messages: &'a [ChatMessage],
    from: usize,
) -> Vec<ToolUseResultPair<'a>> {
    let mut pairs = Vec::new();
    let mut i = from;
    while i < messages.len() {
        if messages[i].role != "assistant" {
            i += 1;
            continue;
        }
        let tool_calls = match &messages[i].tool_calls {
            Some(tcs) if !tcs.is_empty() => tcs,
            _ => {
                i += 1;
                continue;
            }
        };
        i += 1;
        let mut results: HashMap<&str, &ChatMessage> = HashMap::new();
        while i < messages.len() && messages[i].role == "tool" {
            if let Some(id) = messages[i].tool_call_id.as_deref() {
                results.insert(id, &messages[i]);
            }
            i += 1;
        }
        for tc in tool_calls {
            if let Some(result) = results.get(tc.id.as_str()) {
                pairs.push(ToolUseResultPair { call: tc, result });
            }
        }
    }
    pairs
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::message::tool_result_message;

    #[test]
    fn collect_pairs_in_order() {
        let messages = vec![
            ChatMessage {
                role: "assistant".into(),
                content: String::new(),
                tool_calls: Some(vec![
                    ToolCallRecord {
                        id: "a".into(),
                        name: "Read".into(),
                        arguments: serde_json::json!({"file_path": "a.md"}),
                    },
                    ToolCallRecord {
                        id: "b".into(),
                        name: "Edit".into(),
                        arguments: serde_json::json!({"file_path": "a.md"}),
                    },
                ]),
                ..Default::default()
            },
            tool_result_message("a", "read ok"),
            tool_result_message("b", "edit ok"),
        ];
        let pairs = collect_tool_use_result_pairs(&messages, 0);
        assert_eq!(pairs.len(), 2);
        assert_eq!(pairs[0].call.name, "Read");
        assert_eq!(pairs[1].call.name, "Edit");
    }
}
