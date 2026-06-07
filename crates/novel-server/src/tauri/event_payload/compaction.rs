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
            epoch,
            retained_min_turn,
            retained_max_turn,
        } => {
            let mut payload = serde_json::json!({
                "attempt": attempt,
                "action": "done",
                "tokensBefore": tokens_before,
                "tokensAfter": tokens_after,
                "epoch": epoch,
            });
            if let (Some(min), Some(max)) = (retained_min_turn, retained_max_turn) {
                if let Some(obj) = payload.as_object_mut() {
                    obj.insert("retainedMinTurn".into(), serde_json::json!(min));
                    obj.insert("retainedMaxTurn".into(), serde_json::json!(max));
                }
            }
            payload
        }
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
