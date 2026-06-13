//! Read-cache replay cutoff for compaction and resume.

use crate::message::chat_slice_to_compaction;
use crate::ChatMessage;

pub(crate) fn messages_replay_cutoff_chat(messages: &[ChatMessage]) -> usize {
    let compact = chat_slice_to_compaction(messages);
    novel_compaction::messages_replay_cutoff(&compact)
}

#[cfg(test)]
mod tests {
    use super::*;
    use novel_compaction::CONTEXT_REFRESH_USER_PREFIX;

    #[test]
    fn cutoff_after_last_context_refresh() {
        let messages = vec![
            ChatMessage {
                role: "system".into(),
                content: "sys".into(),
                ..Default::default()
            },
            ChatMessage {
                role: "user".into(),
                content: format!("{CONTEXT_REFRESH_USER_PREFIX}\nold"),
                ..Default::default()
            },
            ChatMessage {
                role: "user".into(),
                content: "turn".into(),
                ..Default::default()
            },
        ];
        assert_eq!(messages_replay_cutoff_chat(&messages), 2);
    }
}
