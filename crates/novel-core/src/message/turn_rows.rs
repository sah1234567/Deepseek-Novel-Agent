use crate::turn::{MSG_SEQ_USER, SUB_AGENT_REPORT_PREFIX};
use crate::ChatMessage;

const CONTEXT_REFRESH_PREFIX: &str = "[上下文刷新]";

fn is_mid_turn_user_injection(msg: &ChatMessage) -> bool {
    msg.content.starts_with(SUB_AGENT_REPORT_PREFIX)
}

/// Assign `(turn_number, sequence)` for one chat row (same rules as DB persistence).
pub fn assign_message_turn_seq(
    msg: &ChatMessage,
    turn: &mut i32,
    seq_in_turn: &mut i32,
) -> (i32, i32) {
    if msg.role == "system" {
        *turn = 0;
        *seq_in_turn = 0;
        (0, 0)
    } else if msg.content.starts_with(CONTEXT_REFRESH_PREFIX) {
        (0, 1)
    } else if is_mid_turn_user_injection(msg) {
        *seq_in_turn += 1;
        (*turn, *seq_in_turn)
    } else if msg.role == "user" {
        *turn += 1;
        *seq_in_turn = 0;
        (*turn, MSG_SEQ_USER)
    } else {
        *seq_in_turn += 1;
        (*turn, *seq_in_turn)
    }
}

/// Min/max user turn numbers among messages at or after `from_index` (pre-compaction numbering).
pub fn retained_turn_bounds_from_index(
    messages: &[ChatMessage],
    from_index: usize,
) -> Option<(i32, i32)> {
    let mut turn = 0i32;
    let mut seq_in_turn = 0i32;
    let mut min_t: Option<i32> = None;
    let mut max_t: Option<i32> = None;
    for (i, msg) in messages.iter().enumerate() {
        let (t, _) = assign_message_turn_seq(msg, &mut turn, &mut seq_in_turn);
        if i >= from_index && t >= 1 {
            min_t = Some(min_t.map(|m| m.min(t)).unwrap_or(t));
            max_t = Some(max_t.map(|m| m.max(t)).unwrap_or(t));
        }
    }
    match (min_t, max_t) {
        (Some(min), Some(max)) => Some((min, max)),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ChatMessage;

    #[test]
    fn retained_bounds_use_pre_compaction_turn_numbers() {
        let messages = vec![
            ChatMessage {
                role: "system".into(),
                content: "sys".into(),
                ..Default::default()
            },
            ChatMessage {
                role: "user".into(),
                content: "[上下文刷新]\nx".into(),
                ..Default::default()
            },
            ChatMessage {
                role: "user".into(),
                content: "t1".into(),
                ..Default::default()
            },
            ChatMessage {
                role: "assistant".into(),
                content: "a1".into(),
                ..Default::default()
            },
            ChatMessage {
                role: "user".into(),
                content: "t2".into(),
                ..Default::default()
            },
        ];
        assert_eq!(retained_turn_bounds_from_index(&messages, 4), Some((2, 2)));
        assert_eq!(retained_turn_bounds_from_index(&messages, 2), Some((1, 2)));
    }
}
