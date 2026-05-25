use crate::message_format::format_for_summary;
use crate::message_types::CompactionMessage;
use crate::react_cycles::{SKILL_USER_PREFIX, SUMMARY_USER_PREFIX};
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

pub fn wrap_summary_user_message(summary: &str) -> CompactionMessage {
    CompactionMessage {
        role: "user".into(),
        content: format!("{SUMMARY_USER_PREFIX}\n{summary}"),
        ..Default::default()
    }
}

pub fn wrap_skill_user_message(skill_bodies: &str) -> CompactionMessage {
    CompactionMessage {
        role: "user".into(),
        content: format!("{SKILL_USER_PREFIX}\n{skill_bodies}"),
        ..Default::default()
    }
}

pub struct SessionRebuildInput<'a> {
    pub system: CompactionMessage,
    pub to_summarize: &'a [CompactionMessage],
    pub to_retain: &'a [CompactionMessage],
    pub summary_text: &'a str,
    pub skill_bodies: &'a str,
    /// Deduped invoked skill IDs; when non-empty, `[激活 Skill]` is always inserted.
    pub invoked_skill_ids: &'a [String],
    pub retain: &'a RetainPolicy,
    pub project_root: &'a Path,
}

/// Assemble post-compaction session: system → skill (if invoked) → summary → retained react.
pub fn rebuild_session_messages(input: SessionRebuildInput<'_>) -> Vec<CompactionMessage> {
    let mut retain: Vec<CompactionMessage> = input.to_retain.to_vec();
    apply_level1_on_compaction_messages(&mut retain, input.project_root, input.retain.recent_chapters_full);

    let mut out = vec![input.system];
    if !input.invoked_skill_ids.is_empty() {
        out.push(wrap_skill_user_message(input.skill_bodies));
    }
    out.push(wrap_summary_user_message(input.summary_text));
    out.extend(retain);
    out
}

/// Level 4 fallback: drop oldest retained turns until under budget.
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
        let tokens: usize = rebuilt
            .iter()
            .map(|m| estimate_tokens(&m.content))
            .sum();
        if (tokens as f32 / window as f32) < 0.8 {
            if tokens > window {
                return Err(CompactionError::ContextTooLarge {
                    tokens,
                    window,
                });
            }
            return Ok(rebuilt);
        }
        if turns == 0 {
            if tokens > window {
                return Err(CompactionError::ContextTooLarge {
                    tokens,
                    window,
                });
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
    fn rebuild_order_system_skill_summary_retain() {
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
        assert_eq!(out.len(), 4);
        assert_eq!(out[0].role, "system");
        assert!(out[1].content.starts_with(SKILL_USER_PREFIX));
        assert!(out[2].content.starts_with(SUMMARY_USER_PREFIX));
        assert_eq!(out[3].content, "recent");
    }

    #[test]
    fn rebuild_skill_block_includes_grouped_references() {
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
    fn rebuild_skips_skill_when_no_invoked_ids() {
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
        assert!(out[1].content.starts_with(SUMMARY_USER_PREFIX));
    }
}
