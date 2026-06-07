//! ChatMessage → DB JSON → LLM API conversions and tool-chain repair.

mod convert;
mod display;
mod llm;
mod missing_blocks;
mod repair;
pub mod turn_rows;

pub use convert::{
    chat_slice_to_compaction, chat_to_compaction, compaction_slice_to_chat, parse_tool_call_input,
};
pub use display::{chat_to_json, chat_to_json_for_persist, stored_message_display_text};
pub use llm::{
    assistant_from_completion, stored_to_chat, to_llm_messages, to_llm_messages_traced,
    tool_result_message,
};
pub use missing_blocks::yield_missing_tool_result_blocks;
pub use repair::{repair_tool_use_chain, RepairTraceContext};

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{ChatMessage, ToolCallRecord};
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
    fn stored_to_chat_ignores_display_content() {
        let merged =
            crate::permission::prepend_permission_notice("[权限模式: 无人值守]\nintro", "作者正文");
        let json = chat_to_json_for_persist(
            &ChatMessage {
                role: "user".into(),
                content: merged.clone(),
                tool_call_id: None,
                tool_calls: None,
                reasoning_content: None,
            },
            Some("作者正文"),
        );
        let stored = novel_state::StoredMessage {
            id: "m0".into(),
            session_id: "s".into(),
            turn_number: 1,
            sequence: 0,
            role: "user".into(),
            content_json: json,
            cache_hit_tokens: 0,
            cache_miss_tokens: 0,
            completion_tokens: 0,
            estimated_tokens: None,
            created_at: chrono::Utc::now(),
        };
        let chat = stored_to_chat(&[stored]).unwrap();
        assert_eq!(chat[0].content, merged);
    }

    #[test]
    fn stored_message_display_text_prefers_display_content_field() {
        let json = serde_json::json!({
            "content": "[权限模式: 无人值守]\n\n---\n\n作者",
            "display_content": "作者"
        });
        assert_eq!(stored_message_display_text(&json), "作者");
    }

    #[test]
    fn stored_to_chat_rejects_corrupt_tool_calls() {
        let stored = novel_state::StoredMessage {
            id: "m-bad".into(),
            session_id: "s".into(),
            turn_number: 1,
            sequence: 1,
            role: "assistant".into(),
            content_json: serde_json::json!({
                "content": "x",
                "tool_calls": "not-an-array"
            }),
            cache_hit_tokens: 0,
            cache_miss_tokens: 0,
            completion_tokens: 0,
            estimated_tokens: None,
            created_at: chrono::Utc::now(),
        };
        let err = stored_to_chat(&[stored]).unwrap_err();
        assert!(matches!(
            err,
            crate::AgentError::State(novel_state::StateError::Json(_))
        ));
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
        let chat = stored_to_chat(&[stored]).unwrap();
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
