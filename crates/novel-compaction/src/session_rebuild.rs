use crate::message_format::format_for_summary;
use crate::message_types::CompactionMessage;
use crate::react_cycles::CONTEXT_REFRESH_USER_PREFIX;
use crate::retain_policy::RetainPolicy;
use crate::{estimate_tokens, CompactionError};

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
}

/// Assemble post-compaction session: system → [上下文刷新] user → retained react.
pub fn rebuild_session_messages(input: SessionRebuildInput<'_>) -> Vec<CompactionMessage> {
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
    out.extend(input.to_retain.iter().cloned());
    out
}

/// Inputs for [`rebuild_session_under_budget`].
pub struct SessionBudgetRebuildInput<'a> {
    pub system: CompactionMessage,
    pub summary_text: &'a str,
    pub retain: Vec<CompactionMessage>,
    pub skill_bodies: &'a str,
    pub invoked_skill_ids: &'a [String],
    pub retain_policy: &'a RetainPolicy,
    pub window: usize,
    pub compaction_threshold: f32,
}

/// Budget ratio is `compaction_threshold * 0.5` (e.g. default 0.8 → stop trimming below 40% of window).
pub(crate) fn tokens_under_compaction_budget(
    tokens: usize,
    window: usize,
    compaction_threshold: f32,
) -> bool {
    let budget_ratio = compaction_threshold * 0.5;
    (tokens as f32 / window as f32) < budget_ratio
}

fn shrink_retain_tail(
    retain: &mut Vec<CompactionMessage>,
    turns: &mut usize,
    _retain_policy: &RetainPolicy,
) {
    if *turns == 0 {
        retain.clear();
        return;
    }
    let ranges = crate::react_cycles::user_turn_ranges(retain);
    if ranges.is_empty() {
        retain.clear();
        return;
    }
    if ranges.len() <= *turns {
        let drop_to = ranges.first().map(|r| r.0).unwrap_or(0);
        if drop_to >= retain.len() {
            retain.clear();
        } else {
            *retain = retain.split_off(drop_to);
        }
        return;
    }
    let drop_to = ranges[ranges.len() - *turns].0;
    *retain = retain.split_off(drop_to);
}

/// Rebuild session messages; drop oldest retained turns until estimated tokens are under budget.
pub fn rebuild_session_under_budget(
    input: SessionBudgetRebuildInput<'_>,
) -> Result<Vec<CompactionMessage>, CompactionError> {
    let mut retain = input.retain;
    let mut turns = input.retain_policy.recent_react_turns;
    loop {
        let rebuild = SessionRebuildInput {
            system: input.system.clone(),
            to_summarize: &[],
            to_retain: &retain,
            summary_text: input.summary_text,
            skill_bodies: input.skill_bodies,
            invoked_skill_ids: input.invoked_skill_ids,
            retain: input.retain_policy,
        };
        let rebuilt = rebuild_session_messages(rebuild);
        let tokens: usize = rebuilt.iter().map(|m| estimate_tokens(&m.content)).sum();
        if tokens_under_compaction_budget(tokens, input.window, input.compaction_threshold) {
            if tokens > input.window {
                return Err(CompactionError::ContextTooLarge {
                    tokens,
                    window: input.window,
                });
            }
            return Ok(rebuilt);
        }
        if turns == 0 {
            if tokens > input.window {
                return Err(CompactionError::ContextTooLarge {
                    tokens,
                    window: input.window,
                });
            }
            return Ok(rebuilt);
        }
        turns -= 1;
        shrink_retain_tail(&mut retain, &mut turns, input.retain_policy);
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
        });
        assert_eq!(out.len(), 2);
        assert!(out[1].content.starts_with(CONTEXT_REFRESH_USER_PREFIX));
        assert!(!out[1].content.contains("## 激活 Skill"));
        assert!(out[1].content.contains("## 会话历史摘要"));
    }

    #[test]
    fn tokens_under_compaction_budget_ratio() {
        assert!(tokens_under_compaction_budget(100, 1000, 0.8));
        assert!(!tokens_under_compaction_budget(500, 1000, 0.8));
    }

    #[test]
    fn rebuild_session_under_budget_trims_when_over_ratio() {
        let system = CompactionMessage {
            role: "system".into(),
            content: "sys".into(),
            ..Default::default()
        };
        let retain: Vec<CompactionMessage> = (0..6)
            .map(|i| CompactionMessage {
                role: if i % 2 == 0 { "user" } else { "assistant" }.into(),
                content: "x".repeat(200),
                ..Default::default()
            })
            .collect();
        let out = rebuild_session_under_budget(SessionBudgetRebuildInput {
            system,
            summary_text: "sum",
            retain,
            skill_bodies: "",
            invoked_skill_ids: &[],
            retain_policy: &RetainPolicy {
                recent_react_turns: 1,
                ..RetainPolicy::default()
            },
            window: 1000,
            compaction_threshold: 0.8,
        })
        .expect("rebuild");
        assert!(out.len() >= 2);
    }
}
