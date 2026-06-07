use novel_core::{CompactionAction, Event};

pub(crate) fn compaction_progress_payload(
    attempt: u32,
    action: &CompactionAction,
) -> serde_json::Value {
    match action {
        CompactionAction::Started => {
            serde_json::json!({ "attempt": attempt, "action": "started" })
        }
        CompactionAction::GeneratingSummary => {
            serde_json::json!({ "attempt": attempt, "action": "generating-summary" })
        }
        CompactionAction::RebuildingSession => {
            serde_json::json!({ "attempt": attempt, "action": "rebuilding-session" })
        }
        CompactionAction::Done {
            tokens_before,
            tokens_after,
        } => serde_json::json!({
            "attempt": attempt,
            "action": "done",
            "tokensBefore": tokens_before,
            "tokensAfter": tokens_after,
        }),
        CompactionAction::Failed { reason } => serde_json::json!({
            "attempt": attempt,
            "action": "failed",
            "reason": reason,
        }),
    }
}

pub(crate) fn compaction_payload(event: &Event) -> Option<(String, serde_json::Value)> {
    match event {
        Event::CompactionProgress { attempt, action } => Some((
            "compaction-progress".into(),
            compaction_progress_payload(*attempt, action),
        )),
        _ => None,
    }
}
