use crate::message_format::format_for_summary;
use crate::message_types::CompactionMessage;
use crate::react_cycles::CONTEXT_REFRESH_USER_PREFIX;
use crate::retain_policy::RetainPolicy;
use crate::{apply_level1_on_compaction_messages, estimate_tokens, CompactionError};
use std::path::Path;

/// Rule-based fallback when LLM summarization fails or is unavailable.
pub fn rule_based_summary(messages: &[CompactionMessage], max_chars: usize) -> String {
    let middle = format_for_summary(messages);
    let prefix = "[规则摘要] ";
    let budget = max_chars.saturating_sub(prefix.chars().count());
    let body: String = middle.chars().take(budget).collect();
    format!("{prefix}{body}")
}

/// Merge activated skill bodies and session summary into a single `[上下文刷新]` user message.
pub fn wrap_context_refresh_user_message(
    skill_bodies: &str,
    summary_text: &str,
) -> CompactionMessage {
    let mut sections = Vec::new();
    if !skill_bodies.trim().is_empty() {
        sections.push(format!("## 激活 Skill\n{skill_bodies}"));
    }
    sections.push(format!("## 会话历史摘要\n{summary_text}"));
    CompactionMessage {
        role: "user".into(),
        content: format!("{CONTEXT_REFRESH_USER_PREFIX}\n{}", sections.join("\n\n")),
        ..Default::default()
    }
}

pub struct SessionRebuildInput<'a> {
    pub system: CompactionMessage,
    pub to_summarize: &'a [CompactionMessage],
    pub to_retain: &'a [CompactionMessage],
    pub summary_text: &'a str,
    pub skill_bodies: &'a str,
    /// Deduped invoked skill IDs; skill section omitted when empty.
    pub invoked_skill_ids: &'a [String],
    pub retain: &'a RetainPolicy,
    pub project_root: &'a Path,
}

/// Assemble post-compaction session: system → [上下文刷新] user → retained react.
pub fn rebuild_session_messages(input: SessionRebuildInput<'_>) -> Vec<CompactionMessage> {
    let mut retain: Vec<CompactionMessage> = input.to_retain.to_vec();
    apply_level1_on_compaction_messages(
        &mut retain,
        input.project_root,
        input.retain.recent_chapters_full,
    );

    let skill_block = if input.invoked_skill_ids.is_empty() {
        String::new()
    } else {
        input.skill_bodies.to_string()
    };
    let mut out = vec![input.system];
    out.push(wrap_context_refresh_user_message(
        &skill_block,
        input.summary_text,
    ));
    out.extend(retain);
    out
}

/// Level 4 fallback: drop oldest retained turns until under budget.
#[allow(clippy::too_many_arguments)]
pub fn apply_level4_compaction(
    system: CompactionMessage,
    summary_text: &str,
    mut retain: Vec<CompactionMessage>,
    skill_bodies: &str,
    invoked_skill_ids: &[String],
    retain_policy: &RetainPolicy,
    project_root: &Path,
    window: usize,
) -> Result<Vec<CompactionMessage>, CompactionError> {
    let mut turns = retain_policy.recent_react_turns;
    loop {
        let input = SessionRebuildInput {
            system: system.clone(),
            to_summarize: &[],
            to_retain: &retain,
            summary_text,
            skill_bodies,
            invoked_skill_ids,
            retain: retain_policy,
            project_root,
        };
        let rebuilt = rebuild_session_messages(input);
        let tokens: usize = rebuilt.iter().map(|m| estimate_tokens(&m.content)).sum();
        if (tokens as f32 / window as f32) < 0.8 {
            if tokens > window {
                return Err(CompactionError::ContextTooLarge { tokens, window });
            }
            return Ok(rebuilt);
        }
        if turns == 0 {
            if tokens > window {
                return Err(CompactionError::ContextTooLarge { tokens, window });
            }
            return Ok(rebuilt);
        }
        turns -= 1;
        let ranges = crate::react_cycles::user_turn_ranges(&retain);
        if ranges.is_empty() {
            retain.clear();
            continue;
        }
        if ranges.len() <= turns {
            let drop_to = ranges.first().map(|r| r.0).unwrap_or(0);
            if drop_to >= retain.len() {
                retain.clear();
            } else {
                retain = retain.split_off(drop_to);
            }
            continue;
        }
        let drop_to = ranges[ranges.len() - turns].0;
        retain = retain.split_off(drop_to);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::message_types::CompactionMessage;

    #[test]
    fn rebuild_order_system_context_refresh_retain() {
        let system = CompactionMessage {
            role: "system".into(),
            content: "sys".into(),
            ..Default::default()
        };
        let retain = vec![CompactionMessage {
            role: "user".into(),
            content: "recent".into(),
            ..Default::default()
        }];
        let skill_ids = vec!["x".into()];
        let out = rebuild_session_messages(SessionRebuildInput {
            system,
            to_summarize: &[],
            to_retain: &retain,
            summary_text: "sum",
            skill_bodies: "### x\nbody",
            invoked_skill_ids: &skill_ids,
            retain: &RetainPolicy::default(),
            project_root: Path::new("."),
        });
        assert_eq!(out.len(), 3);
        assert_eq!(out[0].role, "system");
        assert!(out[1].content.starts_with(CONTEXT_REFRESH_USER_PREFIX));
        assert!(out[1].content.contains("## 激活 Skill"));
        assert!(out[1].content.contains("## 会话历史摘要"));
        assert_eq!(out[2].content, "recent");
    }

    #[test]
    fn rebuild_context_refresh_includes_grouped_references() {
        let system = CompactionMessage {
            role: "system".into(),
            content: "sys".into(),
            ..Default::default()
        };
        let skill_ids = vec!["apocalypse".into()];
        let skill_bodies = "### apocalypse\nbody\n\n### apocalypse/references/zombie.md\nref\n";
        let out = rebuild_session_messages(SessionRebuildInput {
            system,
            to_summarize: &[],
            to_retain: &[],
            summary_text: "sum",
            skill_bodies,
            invoked_skill_ids: &skill_ids,
            retain: &RetainPolicy::default(),
            project_root: Path::new("."),
        });
        assert!(out[1].content.contains("apocalypse/references/zombie.md"));
    }

    #[test]
    fn rebuild_omits_skill_section_when_no_invoked_ids() {
        let system = CompactionMessage {
            role: "system".into(),
            content: "sys".into(),
            ..Default::default()
        };
        let out = rebuild_session_messages(SessionRebuildInput {
            system,
            to_summarize: &[],
            to_retain: &[],
            summary_text: "sum",
            skill_bodies: "",
            invoked_skill_ids: &[],
            retain: &RetainPolicy::default(),
            project_root: Path::new("."),
        });
        assert_eq!(out.len(), 2);
        assert!(out[1].content.starts_with(CONTEXT_REFRESH_USER_PREFIX));
        assert!(!out[1].content.contains("## 激活 Skill"));
        assert!(out[1].content.contains("## 会话历史摘要"));
    }
}
