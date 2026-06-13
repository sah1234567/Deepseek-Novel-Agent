use crate::message_types::CompactionMessage;

pub const CONTEXT_REFRESH_USER_PREFIX: &str = "[上下文刷新]";

/// System-injected user messages that must not start a user turn boundary.
pub fn is_user_turn_start(msg: &CompactionMessage) -> bool {
    if msg.role != "user" {
        return false;
    }
    let c = msg.content.as_str();
    if c.starts_with(CONTEXT_REFRESH_USER_PREFIX)
        || c.starts_with("[压缩]")
        || c.starts_with("[Request interrupted")
        || c.starts_with("[子 Agent")
    {
        return false;
    }
    true
}

/// Ranges `[start, end)` for each real user turn (from user message through next user).
pub fn user_turn_ranges(messages: &[CompactionMessage]) -> Vec<(usize, usize)> {
    let starts: Vec<usize> = messages
        .iter()
        .enumerate()
        .filter(|(_, m)| is_user_turn_start(m))
        .map(|(i, _)| i)
        .collect();
    if starts.is_empty() {
        return Vec::new();
    }
    let mut ranges = Vec::with_capacity(starts.len());
    for (idx, &start) in starts.iter().enumerate() {
        let end = starts.get(idx + 1).copied().unwrap_or(messages.len());
        ranges.push((start, end));
    }
    ranges
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PartitionResult {
    /// Index after system message (1) through start of retained region.
    pub summarize_from: usize,
    pub summarize_to: usize,
    /// Start index of retained user turns (inclusive).
    pub retain_from: usize,
}

/// Split messages for compaction: system stays separate; middle → summary; last N user turns retained.
pub fn partition_messages(messages: &[CompactionMessage], keep_turns: usize) -> PartitionResult {
    if messages.is_empty() {
        return PartitionResult {
            summarize_from: 0,
            summarize_to: 0,
            retain_from: 0,
        };
    }
    let body_start = if messages.first().is_some_and(|m| m.role == "system") {
        1
    } else {
        0
    };
    // Skip merged [上下文刷新] user at index 1 when present.
    let body_start = if messages
        .get(body_start)
        .is_some_and(|m| m.role == "user" && m.content.starts_with(CONTEXT_REFRESH_USER_PREFIX))
    {
        body_start + 1
    } else {
        body_start
    };
    let ranges = user_turn_ranges(messages);
    if ranges.len() <= keep_turns {
        return PartitionResult {
            summarize_from: body_start,
            summarize_to: body_start,
            retain_from: body_start,
        };
    }
    let retain_from = ranges[ranges.len() - keep_turns].0;
    PartitionResult {
        summarize_from: body_start,
        summarize_to: retain_from,
        retain_from,
    }
}

/// Index of the first message to replay for read-cache rebuild.
///
/// - If any `[上下文刷新]` user exists: index after the **last** one.
/// - Else: after system (index 1), or 0 when there is no system message.
pub fn messages_replay_cutoff(messages: &[CompactionMessage]) -> usize {
    if messages.is_empty() {
        return 0;
    }
    let mut cutoff = if messages.first().is_some_and(|m| m.role == "system") {
        1
    } else {
        0
    };
    for (i, msg) in messages.iter().enumerate() {
        if msg.role == "user" && msg.content.starts_with(CONTEXT_REFRESH_USER_PREFIX) {
            cutoff = i + 1;
        }
    }
    cutoff
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::message_types::{CompactionMessage, CompactionToolCall};

    fn user(content: &str) -> CompactionMessage {
        CompactionMessage {
            role: "user".into(),
            content: content.into(),
            ..Default::default()
        }
    }

    fn assistant_tools(name: &str) -> CompactionMessage {
        CompactionMessage {
            role: "assistant".into(),
            content: String::new(),
            tool_calls: Some(vec![CompactionToolCall {
                id: "c1".into(),
                name: name.into(),
                arguments: "{}".into(),
            }]),
            ..Default::default()
        }
    }

    fn tool(content: &str) -> CompactionMessage {
        CompactionMessage {
            role: "tool".into(),
            content: content.into(),
            tool_call_id: Some("c1".into()),
            ..Default::default()
        }
    }

    #[test]
    fn partition_keeps_last_n_user_turns() {
        let messages = vec![
            CompactionMessage {
                role: "system".into(),
                content: "sys".into(),
                ..Default::default()
            },
            user("turn1"),
            assistant_tools("Read"),
            tool("file1"),
            user("turn2"),
            user("turn3"),
            user("turn4"),
            user("turn5"),
            user("turn6"),
        ];
        let p = partition_messages(&messages, 3);
        assert_eq!(p.summarize_from, 1);
        assert_eq!(p.summarize_to, 6);
        assert_eq!(p.retain_from, 6);
        assert_eq!(messages[p.retain_from..].len(), 3);
    }

    #[test]
    fn partition_skips_context_refresh_user() {
        let messages = vec![
            CompactionMessage {
                role: "system".into(),
                content: "sys".into(),
                ..Default::default()
            },
            CompactionMessage {
                role: "user".into(),
                content: format!(
                    "{CONTEXT_REFRESH_USER_PREFIX}\n## 激活 Skill\nx\n\n## 会话历史摘要\nold"
                ),
                ..Default::default()
            },
            user("turn1"),
            user("turn2"),
            user("turn3"),
        ];
        let p = partition_messages(&messages, 1);
        assert_eq!(p.summarize_from, 2);
        assert_eq!(p.retain_from, 4);
    }

    #[test]
    fn messages_replay_cutoff_after_last_context_refresh() {
        let messages = vec![
            CompactionMessage {
                role: "system".into(),
                content: "sys".into(),
                ..Default::default()
            },
            CompactionMessage {
                role: "user".into(),
                content: format!("{CONTEXT_REFRESH_USER_PREFIX}\nold"),
                ..Default::default()
            },
            user("turn1"),
            assistant_tools("Read"),
            tool("file1"),
            CompactionMessage {
                role: "user".into(),
                content: format!("{CONTEXT_REFRESH_USER_PREFIX}\nnew"),
                ..Default::default()
            },
            user("turn2"),
            assistant_tools("Edit"),
            tool("ok"),
        ];
        assert_eq!(messages_replay_cutoff(&messages), 6);
    }

    #[test]
    fn messages_replay_cutoff_without_refresh_starts_after_system() {
        let messages = vec![
            CompactionMessage {
                role: "system".into(),
                content: "sys".into(),
                ..Default::default()
            },
            user("turn1"),
            assistant_tools("Read"),
            tool("file1"),
        ];
        assert_eq!(messages_replay_cutoff(&messages), 1);
    }

    #[test]
    fn context_refresh_not_turn_boundary() {
        let messages = vec![
            CompactionMessage {
                role: "system".into(),
                content: "sys".into(),
                ..Default::default()
            },
            CompactionMessage {
                role: "user".into(),
                content: format!("{CONTEXT_REFRESH_USER_PREFIX}\nold summary"),
                ..Default::default()
            },
            user("real turn"),
        ];
        let ranges = user_turn_ranges(&messages);
        assert_eq!(ranges.len(), 1);
        assert_eq!(ranges[0].0, 2);
    }
}
