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

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct UiTurnBounds {
    pub min_turn: i32,
    pub max_turn: i32,
}

impl From<novel_state::TurnBounds> for UiTurnBounds {
    fn from(bounds: novel_state::TurnBounds) -> Self {
        Self {
            min_turn: bounds.min_turn,
            max_turn: bounds.max_turn,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct UiTurnBundle {
    pub turn_number: i32,
    pub messages: Vec<UiMessage>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct ArchiveEpochBounds {
    pub epoch: i32,
    pub bounds: UiTurnBounds,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct SessionTranscriptLayout {
    pub has_context_refresh: bool,
    pub active: UiTurnBounds,
    pub archives: Vec<ArchiveEpochBounds>,
}

const SUB_AGENT_REPORT_PREFIX: &str = "[子 Agent 完成:";
const CONTEXT_REFRESH_PREFIX: &str = "[上下文刷新]";

fn collect_tool_names(content_json: &serde_json::Value, tool_names: &mut HashMap<String, String>) {
    let Some(tcs) = content_json.get("tool_calls").and_then(|v| v.as_array()) else {
        return;
    };
    for tc in tcs {
        let id = tc.get("id").and_then(|v| v.as_str());
        let name = tc.get("name").and_then(|v| v.as_str());
        if let (Some(id), Some(name)) = (id, name) {
            tool_names.insert(id.to_string(), name.to_string());
        }
    }
}

fn build_tool_names_from_rows<'a>(
    rows: impl IntoIterator<Item = (&'a str, &'a serde_json::Value)>,
) -> HashMap<String, String> {
    let mut tool_names = HashMap::new();
    for (role, content_json) in rows {
        if role == "assistant" {
            collect_tool_names(content_json, &mut tool_names);
        }
    }
    tool_names
}

fn detect_message_kind(text: &str) -> Option<String> {
    if text.starts_with(CONTEXT_REFRESH_PREFIX) {
        Some("contextRefresh".into())
    } else if text.starts_with(SUB_AGENT_REPORT_PREFIX) {
        Some("subAgentReport".into())
    } else {
        novel_core::permission_mode_message_kind(text).map(str::to_string)
    }
}

pub fn fork_messages_to_ui(messages: &[novel_state::ForkMessage]) -> Vec<UiMessage> {
    let tool_names =
        build_tool_names_from_rows(messages.iter().map(|m| (m.role.as_str(), &m.content_json)));

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
    let display = if role == "user" {
        novel_core::stored_message_display_text(content_json)
    } else {
        text.clone()
    };
    let resolved_kind = message_kind.or_else(|| {
        if role == "user" {
            detect_message_kind(&text)
        } else {
            None
        }
    });
    let mut blocks = vec![UiContentBlock {
        block_index: 0,
        kind: "text".to_string(),
        text: display,
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
    let ui_role = if resolved_kind.as_deref() == Some("subAgentReport") {
        "subAgentReport".to_string()
    } else {
        role.to_string()
    };
    UiMessage {
        id: id.to_string(),
        role: ui_role,
        tool_name: None,
        fork_run_id,
        message_kind: resolved_kind,
        content_blocks: blocks,
    }
}

pub fn stored_messages_to_ui(stored: &[StoredMessage]) -> Vec<UiMessage> {
    let tool_names =
        build_tool_names_from_rows(stored.iter().map(|m| (m.role.as_str(), &m.content_json)));

    stored
        .iter()
        .filter(|m| m.role == "user" || m.role == "assistant" || m.role == "tool")
        .map(|m| {
            let fork_run_id = m
                .content_json
                .get("fork_run_id")
                .and_then(|v| v.as_str())
                .map(String::from);
            message_row_to_ui(
                &m.id,
                &m.role,
                &m.content_json,
                &tool_names,
                fork_run_id,
                None,
            )
        })
        .collect()
}

pub fn stored_messages_to_turn_bundles(stored: &[StoredMessage]) -> Vec<UiTurnBundle> {
    let mut bundles = Vec::new();
    let mut current_turn: Option<i32> = None;
    let mut chunk: Vec<StoredMessage> = Vec::new();

    let flush = |turn: i32, chunk: &mut Vec<StoredMessage>, bundles: &mut Vec<UiTurnBundle>| {
        if chunk.is_empty() {
            return;
        }
        bundles.push(UiTurnBundle {
            turn_number: turn,
            messages: stored_messages_to_ui(chunk),
        });
        chunk.clear();
    };

    for msg in stored {
        if current_turn != Some(msg.turn_number) {
            if let Some(turn) = current_turn {
                flush(turn, &mut chunk, &mut bundles);
            }
            current_turn = Some(msg.turn_number);
        }
        chunk.push(msg.clone());
    }
    if let Some(turn) = current_turn {
        flush(turn, &mut chunk, &mut bundles);
    }
    bundles
}

pub fn build_session_transcript_layout(
    db: &novel_state::Database,
    session_id: &str,
) -> Result<SessionTranscriptLayout, novel_state::StateError> {
    let has_context_refresh = db.has_active_context_refresh(session_id)?;
    let active = db
        .get_active_turn_bounds(session_id)?
        .map(UiTurnBounds::from)
        .unwrap_or(UiTurnBounds {
            min_turn: 0,
            max_turn: 0,
        });

    let epochs = db.get_archived_epochs(session_id)?;
    let mut archives = Vec::with_capacity(epochs.len());
    for epoch in epochs {
        if let Some(bounds) = db.get_archived_turn_bounds(session_id, epoch)? {
            archives.push(ArchiveEpochBounds {
                epoch,
                bounds: bounds.into(),
            });
        }
    }

    Ok(SessionTranscriptLayout {
        has_context_refresh,
        active,
        archives,
    })
}

pub fn validate_turn_range(from_turn: i32, to_turn: i32) -> Result<(), String> {
    if from_turn > to_turn {
        return Err("fromTurn must be <= toTurn".into());
    }
    Ok(())
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
            stored(
                "assistant",
                "done",
                serde_json::json!({ "content": "done" }),
            ),
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

    #[test]
    fn permission_mode_user_display_strips_injected_prefix() {
        let merged = novel_core::prepend_permission_notice(
            &novel_core::format_enter_unattended_prefix(),
            "继续写第5章",
        );
        let msgs = stored_messages_to_ui(&[stored(
            "user",
            "u1",
            serde_json::json!({
                "content": merged,
                "display_content": "继续写第5章"
            }),
        )]);
        assert_eq!(msgs[0].content_blocks[0].text, "继续写第5章");
        assert!(!msgs[0].content_blocks[0].text.contains("[权限模式:"));
        assert_eq!(msgs[0].message_kind.as_deref(), Some("permissionModeEnter"));
    }

    #[test]
    fn context_refresh_message_kind() {
        let msgs = stored_messages_to_ui(&[stored(
            "user",
            "ctx",
            serde_json::json!({ "content": "[上下文刷新]\n## 会话历史摘要\nsum" }),
        )]);
        assert_eq!(msgs[0].message_kind.as_deref(), Some("contextRefresh"));
        assert_eq!(msgs[0].role, "user");
    }

    #[test]
    fn sub_agent_report_message_kind_and_role() {
        let msgs = stored_messages_to_ui(&[stored(
            "user",
            "report",
            serde_json::json!({ "content": "[子 Agent 完成: KnowledgeAuditor]\n## 报告\nok" }),
        )]);
        assert_eq!(msgs[0].message_kind.as_deref(), Some("subAgentReport"));
        assert_eq!(msgs[0].role, "subAgentReport");
    }

    const FORK_MESSAGES_FIXTURE: &str = r#"[
        {
            "id": "fm-user",
            "run_id": "run-1",
            "sequence": 0,
            "role": "user",
            "content_json": { "content": "audit chapter 1" },
            "created_at": "2024-06-01T12:00:00Z"
        },
        {
            "id": "fm-asst",
            "run_id": "run-1",
            "sequence": 1,
            "role": "assistant",
            "content_json": {
                "content": "",
                "tool_calls": [{ "id": "tc-fork", "name": "Read", "arguments": {} }]
            },
            "created_at": "2024-06-01T12:00:01Z"
        },
        {
            "id": "fm-tool",
            "run_id": "run-1",
            "sequence": 2,
            "role": "tool",
            "content_json": { "content": "file body", "tool_call_id": "tc-fork" },
            "created_at": "2024-06-01T12:00:02Z"
        },
        {
            "id": "fm-system",
            "run_id": "run-1",
            "sequence": 3,
            "role": "system",
            "content_json": { "content": "hidden" },
            "created_at": "2024-06-01T12:00:03Z"
        }
    ]"#;

    #[test]
    fn fork_messages_fixture_maps_tool_name() {
        let stored: Vec<novel_state::ForkMessage> =
            serde_json::from_str(FORK_MESSAGES_FIXTURE).unwrap();
        let ui = fork_messages_to_ui(&stored);
        assert_eq!(ui.len(), 3);
        assert_eq!(ui[0].role, "user");
        assert_eq!(ui[1].role, "assistant");
        assert_eq!(ui[2].role, "tool");
        assert_eq!(ui[2].tool_name.as_deref(), Some("Read"));
        assert_eq!(ui[2].content_blocks[0].text, "file body");
    }

    #[test]
    fn stored_messages_to_turn_bundles_groups_by_turn() {
        let msgs = vec![
            StoredMessage {
                id: "a".into(),
                session_id: "s1".into(),
                turn_number: 1,
                sequence: 0,
                role: "user".into(),
                content_json: serde_json::json!({ "content": "t1" }),
                cache_hit_tokens: 0,
                cache_miss_tokens: 0,
                completion_tokens: 0,
                estimated_tokens: None,
                created_at: Utc::now(),
            },
            StoredMessage {
                id: "b".into(),
                session_id: "s1".into(),
                turn_number: 1,
                sequence: 1,
                role: "assistant".into(),
                content_json: serde_json::json!({ "content": "a1" }),
                cache_hit_tokens: 0,
                cache_miss_tokens: 0,
                completion_tokens: 0,
                estimated_tokens: None,
                created_at: Utc::now(),
            },
            StoredMessage {
                id: "c".into(),
                session_id: "s1".into(),
                turn_number: 2,
                sequence: 0,
                role: "user".into(),
                content_json: serde_json::json!({ "content": "t2" }),
                cache_hit_tokens: 0,
                cache_miss_tokens: 0,
                completion_tokens: 0,
                estimated_tokens: None,
                created_at: Utc::now(),
            },
        ];
        let bundles = stored_messages_to_turn_bundles(&msgs);
        assert_eq!(bundles.len(), 2);
        assert_eq!(bundles[0].turn_number, 1);
        assert_eq!(bundles[0].messages.len(), 2);
        assert_eq!(bundles[1].turn_number, 2);
    }

    #[test]
    fn build_session_transcript_layout_bounds() {
        use tempfile::TempDir;

        let tmp = TempDir::new().unwrap();
        let db = novel_state::Database::open(tmp.path().join("test.db")).unwrap();
        let sid = db.create_session("/tmp/proj", "deepseek-chat").unwrap();
        db.insert_message(
            &sid,
            1,
            0,
            "user",
            &serde_json::json!({ "content": "archived user" }),
            None,
        )
        .unwrap();
        db.archive_session_messages(&sid, 1).unwrap();
        db.replace_session_messages(
            &sid,
            &[(
                2,
                0,
                "user",
                &serde_json::json!({ "content": "active user" }),
            )],
        )
        .unwrap();
        db.insert_message(
            &sid,
            0,
            1,
            "user",
            &serde_json::json!({ "content": "[上下文刷新]\n## 会话历史摘要\nx" }),
            None,
        )
        .unwrap();

        let layout = build_session_transcript_layout(&db, &sid).unwrap();
        assert!(layout.has_context_refresh);
        assert_eq!(layout.active.min_turn, 0);
        assert_eq!(layout.active.max_turn, 2);
        assert_eq!(layout.archives.len(), 1);
        assert_eq!(layout.archives[0].bounds.min_turn, 1);
    }
}
