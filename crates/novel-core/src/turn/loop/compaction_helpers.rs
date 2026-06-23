//! Pure helpers for [`super::compaction::AgentEngine::compact_and_sync`]: skill snapshot,
//! retained-turn bounds, circuit-breaker emit, and compaction display overlay.

use std::collections::HashMap;

use crate::context::dynamic_context::{
    filter_loadable_reference_paths, filter_loadable_skill_ids, format_activated_skill_block,
};
use crate::message::turn_rows::retained_turn_bounds_from_index;
use crate::{AgentEngine, AgentError, ChatMessage, CompactionAction, Event};
use novel_compaction::{user_turn_ranges, CompactionMessage};
use tokio::sync::mpsc;

pub(crate) struct CompactionSkillSnapshot {
    pub skill_ids: Vec<String>,
    pub ref_paths: Vec<String>,
    pub skill_bodies: String,
}

pub(crate) fn emit_compaction_circuit_breaker(
    fail_count: u32,
    event_tx: Option<&mpsc::UnboundedSender<Event>>,
) {
    if let Some(tx) = event_tx {
        let reason = "连续压缩失败已达上限，已暂停自动压缩；请缩短对话或新建会话".into();
        let _ = tx.send(Event::CompactionProgress {
            attempt: fail_count,
            action: CompactionAction::Failed { reason },
        });
    }
}

pub(crate) fn compaction_skill_snapshot(
    engine: &mut AgentEngine,
) -> Result<CompactionSkillSnapshot, AgentError> {
    engine.refresh_system_dynamic_sections()?;
    let skill_ids = filter_loadable_skill_ids(
        &engine.shared.session.project_root,
        &engine.shared.agent_skills_dir,
        &engine.invoked_skill_ids,
    );
    let ref_paths = filter_loadable_reference_paths(
        &engine.shared.session.project_root,
        &engine.shared.agent_skills_dir,
        &engine.read_skill_reference_paths,
        &skill_ids,
    );
    let skill_bodies = format_activated_skill_block(
        &engine.shared.session.project_root,
        &engine.shared.agent_skills_dir,
        &skill_ids,
        &ref_paths,
    );
    Ok(CompactionSkillSnapshot {
        skill_ids,
        ref_paths,
        skill_bodies,
    })
}

pub(crate) fn compaction_retained_turn_bounds(
    final_msgs: &[CompactionMessage],
    compacted: &[CompactionMessage],
    partition_retain_from: usize,
    messages: &[ChatMessage],
) -> Option<(i32, i32)> {
    let kept_turns = if final_msgs.len() > 2 {
        user_turn_ranges(&final_msgs[2..]).len()
    } else {
        0
    };
    if kept_turns == 0 {
        return None;
    }
    let ranges = user_turn_ranges(compacted);
    let start_index = if ranges.len() >= kept_turns {
        ranges[ranges.len() - kept_turns].0
    } else {
        partition_retain_from
    };
    retained_turn_bounds_from_index(messages, start_index)
}

pub(crate) fn overlay_compaction_display_content(
    new_messages: &mut [ChatMessage],
    partition_retain_from: usize,
    display_snapshot: &HashMap<usize, String>,
    retained_in_final: usize,
    prefix_len: usize,
) {
    for offset in 0..retained_in_final {
        let new_idx = prefix_len + offset;
        let Some(msg) = new_messages.get_mut(new_idx) else {
            break;
        };
        if msg.display_content.is_none() {
            let orig_idx = partition_retain_from + offset;
            if let Some(display) = display_snapshot.get(&orig_idx) {
                msg.display_content = Some(display.clone());
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use novel_compaction::CompactionMessage;

    #[test]
    fn retained_bounds_none_when_no_kept_turns() {
        let final_msgs = vec![
            CompactionMessage {
                role: "system".into(),
                content: "s".into(),
                ..Default::default()
            },
            CompactionMessage {
                role: "user".into(),
                content: "refresh".into(),
                ..Default::default()
            },
        ];
        assert!(compaction_retained_turn_bounds(&final_msgs, &[], 0, &[]).is_none());
    }

    #[test]
    fn overlay_copies_display_from_snapshot() {
        let mut msgs = vec![
            ChatMessage {
                role: "user".into(),
                content: "u".into(),
                display_content: None,
                ..Default::default()
            },
            ChatMessage {
                role: "assistant".into(),
                content: "a".into(),
                display_content: None,
                ..Default::default()
            },
        ];
        let mut snap = HashMap::new();
        snap.insert(1, "shown".into());
        overlay_compaction_display_content(&mut msgs, 1, &snap, 1, 1);
        assert_eq!(msgs[1].display_content.as_deref(), Some("shown"));
    }
}
