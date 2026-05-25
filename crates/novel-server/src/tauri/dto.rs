use novel_state::StoredMessage;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct UiContentBlock {
    pub block_index: u32,
    pub kind: String,
    pub text: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct UiMessage {
    pub id: String,
    pub role: String,
    pub content_blocks: Vec<UiContentBlock>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tool_name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub fork_run_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub message_kind: Option<String>,
}

const SUB_AGENT_REPORT_PREFIX: &str = "[子 Agent 完成:";

pub fn fork_messages_to_ui(messages: &[novel_state::ForkMessage]) -> Vec<UiMessage> {
    let mut tool_names: HashMap<String, String> = HashMap::new();
    for m in messages {
        if m.role != "assistant" {
            continue;
        }
        let Some(tcs) = m.content_json.get("tool_calls").and_then(|v| v.as_array()) else {
            continue;
        };
        for tc in tcs {
            let id = tc.get("id").and_then(|v| v.as_str());
            let name = tc.get("name").and_then(|v| v.as_str());
            if let (Some(id), Some(name)) = (id, name) {
                tool_names.insert(id.to_string(), name.to_string());
            }
        }
    }

    messages
        .iter()
        .filter(|m| m.role == "user" || m.role == "assistant" || m.role == "tool")
        .map(|m| message_row_to_ui(&m.id, &m.role, &m.content_json, &tool_names, None, None))
        .collect()
}

fn message_row_to_ui(
    id: &str,
    role: &str,
    content_json: &serde_json::Value,
    tool_names: &HashMap<String, String>,
    fork_run_id: Option<String>,
    message_kind: Option<String>,
) -> UiMessage {
    if role == "tool" {
        let content = content_json
            .get("content")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string();
        let tool_call_id = content_json
            .get("tool_call_id")
            .and_then(|v| v.as_str())
            .unwrap_or("");
        let tool_name = tool_names
            .get(tool_call_id)
            .cloned()
            .unwrap_or_else(|| "Tool".to_string());
        return UiMessage {
            id: id.to_string(),
            role: role.to_string(),
            tool_name: Some(tool_name),
            fork_run_id,
            message_kind,
            content_blocks: vec![UiContentBlock {
                block_index: 0,
                kind: "text".to_string(),
                text: content,
            }],
        };
    }

    let text = content_json
        .get("content")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string();
    let mut blocks = vec![UiContentBlock {
        block_index: 0,
        kind: "text".to_string(),
        text,
    }];
    if let Some(rc) = content_json
        .get("reasoning_content")
        .and_then(|v| v.as_str())
    {
        if !rc.is_empty() {
            blocks.insert(
                0,
                UiContentBlock {
                    block_index: 0,
                    kind: "thinking".to_string(),
                    text: rc.to_string(),
                },
            );
        }
    }
    UiMessage {
        id: id.to_string(),
        role: role.to_string(),
        tool_name: None,
        fork_run_id,
        message_kind,
        content_blocks: blocks,
    }
}

pub fn stored_messages_to_ui(stored: &[StoredMessage]) -> Vec<UiMessage> {
    let mut tool_names: HashMap<String, String> = HashMap::new();
    for m in stored {
        if m.role != "assistant" {
            continue;
        }
        let Some(tcs) = m.content_json.get("tool_calls").and_then(|v| v.as_array()) else {
            continue;
        };
        for tc in tcs {
            let id = tc.get("id").and_then(|v| v.as_str());
            let name = tc.get("name").and_then(|v| v.as_str());
            if let (Some(id), Some(name)) = (id, name) {
                tool_names.insert(id.to_string(), name.to_string());
            }
        }
    }

    stored
        .iter()
        .filter(|m| m.role == "user" || m.role == "assistant" || m.role == "tool")
        .map(|m| {
            if m.role == "tool" {
                let content = m
                    .content_json
                    .get("content")
                    .and_then(|v| v.as_str())
                    .unwrap_or("")
                    .to_string();
                let tool_call_id = m
                    .content_json
                    .get("tool_call_id")
                    .and_then(|v| v.as_str())
                    .unwrap_or("");
                let tool_name = tool_names
                    .get(tool_call_id)
                    .cloned()
                    .unwrap_or_else(|| "Tool".to_string());
                return UiMessage {
                    id: m.id.clone(),
                    role: m.role.clone(),
                    tool_name: Some(tool_name),
                    fork_run_id: None,
                    message_kind: None,
                    content_blocks: vec![UiContentBlock {
                        block_index: 0,
                        kind: "text".to_string(),
                        text: content,
                    }],
                };
            }

            let fork_run_id = m
                .content_json
                .get("fork_run_id")
                .and_then(|v| v.as_str())
                .map(String::from);
            let text = m
                .content_json
                .get("content")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string();
            let message_kind = if text.starts_with(SUB_AGENT_REPORT_PREFIX) {
                Some("subAgentReport".into())
            } else {
                None
            };
            let mut blocks = vec![UiContentBlock {
                block_index: 0,
                kind: "text".to_string(),
                text,
            }];
            if let Some(rc) = m.content_json.get("reasoning_content").and_then(|v| v.as_str()) {
                if !rc.is_empty() {
                    blocks.insert(
                        0,
                        UiContentBlock {
                            block_index: 0,
                            kind: "thinking".to_string(),
                            text: rc.to_string(),
                        },
                    );
                }
            }
            UiMessage {
                id: m.id.clone(),
                role: if message_kind.is_some() {
                    "subAgentReport".to_string()
                } else {
                    m.role.clone()
                },
                tool_name: None,
                fork_run_id,
                message_kind,
                content_blocks: blocks,
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Utc;

    fn stored(role: &str, _content: &str, extra: serde_json::Value) -> StoredMessage {
        StoredMessage {
            id: "test-id".into(),
            session_id: "s1".into(),
            turn_number: 1,
            sequence: 0,
            role: role.into(),
            content_json: extra,
            cache_hit_tokens: 0,
            cache_miss_tokens: 0,
            completion_tokens: 0,
            estimated_tokens: None,
            created_at: Utc::now(),
        }
    }

    #[test]
    fn chronological_user_assistant_tool_order() {
        let msgs = stored_messages_to_ui(&[
            stored("user", "hi", serde_json::json!({ "content": "hi" })),
            stored(
                "assistant",
                "",
                serde_json::json!({
                    "content": "",
                    "tool_calls": [{"id": "tc1", "name": "TodoWrite", "arguments": {}}]
                }),
            ),
            stored(
                "tool",
                "ok",
                serde_json::json!({ "content": "ok", "tool_call_id": "tc1" }),
            ),
            stored("assistant", "done", serde_json::json!({ "content": "done" })),
        ]);
        assert_eq!(msgs.len(), 4);
        assert_eq!(msgs[0].role, "user");
        assert_eq!(msgs[1].role, "assistant");
        assert_eq!(msgs[2].role, "tool");
        assert_eq!(msgs[2].tool_name.as_deref(), Some("TodoWrite"));
        assert_eq!(msgs[3].role, "assistant");
    }

    #[test]
    fn filters_system_role() {
        let msgs = stored_messages_to_ui(&[
            stored("system", "sys", serde_json::json!({ "content": "sys" })),
            stored("user", "hi", serde_json::json!({ "content": "hi" })),
        ]);
        assert_eq!(msgs.len(), 1);
        assert_eq!(msgs[0].role, "user");
    }
}
