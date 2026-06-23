//! Turn-start validation and user message assembly.

use crate::engine::session_llm::SessionLlmSnapshot;
use crate::{AgentError, ChatMessage};
use novel_config::ProjectSettings;

pub(crate) fn validate_turn_start(
    content: &str,
    pending_user_question: bool,
) -> Result<String, AgentError> {
    if content.trim().is_empty() {
        tracing::warn!("handle_message rejected: empty content");
        return Err(AgentError::Validation("empty message".into()));
    }
    if pending_user_question {
        tracing::warn!("handle_message rejected: pending user question");
        return Err(AgentError::Validation(
            "answer pending question before sending a new message".into(),
        ));
    }
    Ok(content.trim().to_string())
}

pub(crate) fn resolve_turn_llm_snapshot(
    model_override: Option<&str>,
    settings: &ProjectSettings,
) -> (SessionLlmSnapshot, bool) {
    let per_turn_override =
        model_override.is_some_and(|m| !m.is_empty() && m != settings.model.model);
    let snap = match model_override {
        Some(m) if !m.is_empty() && m != settings.model.model => SessionLlmSnapshot {
            model: m.to_string(),
            thinking_enabled: settings.model.thinking_enabled,
        },
        _ => SessionLlmSnapshot::from_settings(settings),
    };
    (snap, per_turn_override)
}

pub(crate) fn build_turn_user_message(
    author_content: &str,
    permission_prefix: Option<String>,
) -> ChatMessage {
    let merged_content = if let Some(prefix) = permission_prefix {
        tracing::debug!(
            prefix_len = prefix.len(),
            has_display_content = true,
            "permission_mode_prefix_prepended_to_user_message"
        );
        crate::permission::prepend_permission_notice(&prefix, author_content)
    } else {
        author_content.to_string()
    };
    let display_content = if crate::permission::is_permission_mode_notice(&merged_content) {
        Some(author_content.to_string())
    } else {
        None
    };
    ChatMessage {
        role: "user".into(),
        content: merged_content,
        tool_call_id: None,
        tool_calls: None,
        reasoning_content: None,
        display_content,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use novel_config::ProjectSettings;

    #[test]
    fn rejects_empty_message() {
        assert!(validate_turn_start("  ", false).is_err());
    }

    #[test]
    fn rejects_while_question_pending() {
        assert!(validate_turn_start("hello", true).is_err());
    }

    #[test]
    fn trims_author_content() {
        assert_eq!(validate_turn_start("  hi  ", false).unwrap(), "hi");
    }

    #[test]
    fn model_override_changes_snapshot() {
        let settings = ProjectSettings::default();
        let (snap, overridden) = resolve_turn_llm_snapshot(Some("other-model"), &settings);
        assert!(overridden);
        assert_eq!(snap.model, "other-model");
    }

    #[test]
    fn permission_prefix_sets_display_content() {
        let prefix = crate::permission::format_enter_unattended_prefix();
        let msg = build_turn_user_message("写正文", Some(prefix));
        assert!(msg.display_content.is_some());
        assert!(msg.content.contains("写正文"));
    }
}
