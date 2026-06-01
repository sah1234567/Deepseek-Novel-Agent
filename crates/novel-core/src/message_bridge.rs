use crate::messages::yield_missing_tool_result_blocks;
use crate::{ChatMessage, ToolCallRecord};
use novel_compaction::{CompactionMessage, CompactionToolCall};
use novel_deepseek::{parse_tool_arguments, LlmChatMessage, LlmCompletion, LlmToolCall};
use novel_state::StoredMessage;
use serde_json::Value;
use std::collections::HashSet;

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

/// Ensure tool messages match a preceding assistant `tool_calls` block (API requirement).
pub fn repair_tool_use_chain(messages: &mut Vec<ChatMessage>) {
    let removed = remove_orphan_tool_messages(messages);
    let inserted = fill_missing_tool_results(messages);
    if removed > 0 || inserted > 0 {
        tracing::debug!(
            removed_orphans = removed,
            inserted_stubs = inserted,
            "repaired tool_use chain"
        );
    }
}

fn remove_orphan_tool_messages(messages: &mut Vec<ChatMessage>) -> usize {
    let before = messages.len();
    let mut i = 0;
    while i < messages.len() {
        if messages[i].role != "tool" {
            i += 1;
            continue;
        }
        let tool_id = messages[i].tool_call_id.clone();
        if !is_tool_call_valid_at(messages, i, tool_id.as_deref()) {
            tracing::warn!(
                tool_call_id = ?tool_id,
                index = i,
                "removing orphan tool message without preceding assistant tool_calls"
            );
            messages.remove(i);
            continue;
        }
        i += 1;
    }
    before.saturating_sub(messages.len())
}

fn is_tool_call_valid_at(messages: &[ChatMessage], tool_idx: usize, tool_id: Option<&str>) -> bool {
    let Some(tid) = tool_id else {
        return false;
    };
    let mut block_start = tool_idx;
    while block_start > 0 && messages[block_start - 1].role == "tool" {
        block_start -= 1;
    }
    if block_start == 0 {
        return false;
    }
    let assistant_idx = block_start - 1;
    if messages[assistant_idx].role != "assistant" {
        return false;
    }
    messages[assistant_idx]
        .tool_calls
        .as_ref()
        .is_some_and(|tcs| tcs.iter().any(|tc| tc.id == tid))
}

fn fill_missing_tool_results(messages: &mut Vec<ChatMessage>) -> usize {
    let mut inserted = 0usize;
    let mut i = 0;
    while i < messages.len() {
        if messages[i].role != "assistant" {
            i += 1;
            continue;
        }
        let tool_calls = match &messages[i].tool_calls {
            Some(tcs) if !tcs.is_empty() => tcs.clone(),
            _ => {
                i += 1;
                continue;
            }
        };
        let mut j = i + 1;
        let mut seen = HashSet::new();
        while j < messages.len() && messages[j].role == "tool" {
            if let Some(id) = &messages[j].tool_call_id {
                seen.insert(id.clone());
            }
            j += 1;
        }
        let missing: Vec<_> = tool_calls
            .iter()
            .filter(|tc| !seen.contains(&tc.id))
            .map(|tc| tc.id.as_str())
            .collect();
        if !missing.is_empty() {
            tracing::warn!(
                ?missing,
                "inserting synthetic tool results for missing tool_call ids"
            );
            let assistant = messages[i].clone();
            let stubs = yield_missing_tool_result_blocks(
                &assistant,
                "Error: tool result was not recorded (session repaired)",
            );
            for stub in stubs {
                if stub
                    .tool_call_id
                    .as_ref()
                    .is_some_and(|id| missing.contains(&id.as_str()))
                {
                    messages.insert(j, stub);
                    j += 1;
                    inserted += 1;
                }
            }
        }
        i = j;
    }
    inserted
}

pub fn to_llm_messages(messages: &[ChatMessage]) -> Vec<LlmChatMessage> {
    let mut repaired = messages.to_vec();
    repair_tool_use_chain(&mut repaired);
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

pub fn stored_to_chat(stored: &[StoredMessage]) -> Vec<ChatMessage> {
    stored
        .iter()
        .map(|s| {
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
            let tool_calls = s
                .content_json
                .get("tool_calls")
                .and_then(|v| serde_json::from_value::<Vec<ToolCallRecord>>(v.clone()).ok())
                .filter(|tcs| !tcs.is_empty());
            ChatMessage {
                role: s.role.clone(),
                content,
                tool_call_id,
                tool_calls,
                reasoning_content,
            }
        })
        .collect()
}

pub fn chat_to_json(msg: &ChatMessage) -> Value {
    let mut obj = serde_json::json!({ "content": msg.content });
    if let Some(id) = &msg.tool_call_id {
        obj["tool_call_id"] = Value::String(id.clone());
    }
    if let Some(tcs) = &msg.tool_calls {
        obj["tool_calls"] = serde_json::to_value(tcs).unwrap_or_else(|e| {
            tracing::warn!(%e, "failed to serialize tool_calls to JSON");
            Value::Null
        });
    }
    if let Some(rc) = &msg.reasoning_content {
        if !rc.is_empty() {
            obj["reasoning_content"] = Value::String(rc.clone());
        }
    }
    obj
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

#[cfg(test)]
mod tests {
    use super::*;
    use novel_deepseek::LlmCompletion;

    #[test]
    fn to_llm_messages_preserves_reasoning_content() {
        let msgs = vec![ChatMessage {
            role: "assistant".into(),
            content: "ok".into(),
            tool_call_id: None,
            tool_calls: None,
            reasoning_content: Some("chain-of-thought".into()),
        }];
        let llm = to_llm_messages(&msgs);
        assert_eq!(
            llm[0].reasoning_content.as_deref(),
            Some("chain-of-thought")
        );
    }

    #[test]
    fn stored_to_chat_restores_tool_calls() {
        let json = chat_to_json(&ChatMessage {
            role: "assistant".into(),
            content: "call tools".into(),
            tool_call_id: None,
            tool_calls: Some(vec![ToolCallRecord {
                id: "c1".into(),
                name: "InvokeSkill".into(),
                arguments: serde_json::json!({"skill_id": "rebirth"}),
            }]),
            reasoning_content: None,
        });
        let stored = novel_state::StoredMessage {
            id: "m1".into(),
            session_id: "s".into(),
            turn_number: 1,
            sequence: 1,
            role: "assistant".into(),
            content_json: json,
            cache_hit_tokens: 0,
            cache_miss_tokens: 0,
            completion_tokens: 0,
            estimated_tokens: None,
            created_at: chrono::Utc::now(),
        };
        let chat = stored_to_chat(&[stored]);
        assert_eq!(chat.len(), 1);
        assert_eq!(chat[0].tool_calls.as_ref().unwrap().len(), 1);
        assert_eq!(chat[0].tool_calls.as_ref().unwrap()[0].id, "c1");
    }

    #[test]
    fn repair_removes_orphan_tool_without_assistant_tool_calls() {
        let mut msgs = vec![
            ChatMessage {
                role: "user".into(),
                content: "hi".into(),
                tool_call_id: None,
                tool_calls: None,
                reasoning_content: None,
            },
            ChatMessage {
                role: "assistant".into(),
                content: String::new(),
                tool_call_id: None,
                tool_calls: None,
                reasoning_content: None,
            },
            tool_result_message("orphan", "body"),
        ];
        repair_tool_use_chain(&mut msgs);
        assert!(!msgs.iter().any(|m| m.role == "tool"));
    }

    #[test]
    fn repair_fills_missing_tool_result_for_assistant_tool_calls() {
        let mut msgs = vec![
            ChatMessage {
                role: "assistant".into(),
                content: String::new(),
                tool_call_id: None,
                tool_calls: Some(vec![
                    ToolCallRecord {
                        id: "a".into(),
                        name: "InvokeSkill".into(),
                        arguments: serde_json::json!({}),
                    },
                    ToolCallRecord {
                        id: "b".into(),
                        name: "InvokeSkill".into(),
                        arguments: serde_json::json!({}),
                    },
                ]),
                reasoning_content: None,
            },
            tool_result_message("a", "ok"),
        ];
        repair_tool_use_chain(&mut msgs);
        assert_eq!(msgs.iter().filter(|m| m.role == "tool").count(), 2);
    }

    #[test]
    fn assistant_from_completion_keeps_reasoning() {
        let c = LlmCompletion {
            content: Some("hi".into()),
            tool_calls: vec![],
            usage: None,
            reasoning_content: Some("think".into()),
            stop_reason: None,
        };
        let m = assistant_from_completion(&c);
        assert_eq!(m.reasoning_content.as_deref(), Some("think"));
    }
}
