use crate::message_types::CompactionMessage;

/// Serialize messages for the summarizer LLM, preserving tool metadata.
pub fn format_for_summary(messages: &[CompactionMessage]) -> String {
    messages
        .iter()
        .map(format_one)
        .collect::<Vec<_>>()
        .join("\n---\n")
}

fn format_one(msg: &CompactionMessage) -> String {
    match msg.role.as_str() {
        "assistant" => {
            let tools = msg
                .tool_calls
                .as_ref()
                .map(|tcs| {
                    tcs.iter()
                        .map(|tc| tc.name.as_str())
                        .collect::<Vec<_>>()
                        .join(", ")
                })
                .unwrap_or_default();
            let body = if msg.content.is_empty() {
                String::new()
            } else {
                msg.content.clone()
            };
            if tools.is_empty() {
                format!("[assistant] {body}")
            } else {
                format!("[assistant] {body} | tools: {tools}")
            }
        }
        "tool" => {
            let name = msg.tool_call_id.as_deref().unwrap_or("tool");
            format!("[tool:{name}] {}", truncate(&msg.content, 4000))
        }
        other => format!("[{other}] {}", truncate(&msg.content, 8000)),
    }
}

fn truncate(s: &str, max_chars: usize) -> String {
    if s.chars().count() <= max_chars {
        return s.to_string();
    }
    let removed = s.chars().count().saturating_sub(max_chars);
    let suffix = format!("…[truncated {removed} chars]");
    novel_knowledge::truncate_with_suffix(s, max_chars, &suffix)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::message_types::{CompactionMessage, CompactionToolCall};

    #[test]
    fn assistant_includes_tool_names() {
        let msg = CompactionMessage {
            role: "assistant".into(),
            content: "ok".into(),
            tool_calls: Some(vec![CompactionToolCall {
                id: "1".into(),
                name: "Read".into(),
                arguments: "{}".into(),
            }]),
            ..Default::default()
        };
        let out = format_one(&msg);
        assert!(out.contains("tools: Read"));
    }
}
