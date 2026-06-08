//! Live fork overlay stream subscription set (UI opens SubAgentOverlay).
//!
//! Stored in `AppState` and cloned into `EngineShared` so subagent tasks can gate
//! high-frequency `SubAgentStreamDelta` / tool events without queuing on the engine
//! command channel (drain may block `SendMessage` for minutes).

use crate::Event;
use std::collections::HashSet;
use std::sync::{Arc, RwLock};
use tokio::sync::mpsc;

pub type ForkStreamSubscriptions = Arc<RwLock<HashSet<String>>>;

pub fn new_fork_stream_subscriptions() -> ForkStreamSubscriptions {
    Arc::new(RwLock::new(HashSet::new()))
}

pub fn is_fork_stream_subscribed(subs: &ForkStreamSubscriptions, run_id: &str) -> bool {
    subs.read()
        .map(|guard| guard.contains(run_id))
        .unwrap_or(false)
}

/// Returns `fork_run_id` when `event` is fork-overlay scoped.
fn fork_overlay_run_id(event: &Event) -> Option<&str> {
    match event {
        Event::SubAgentStreamDelta { fork_run_id, .. }
        | Event::SubAgentToolUpdate { fork_run_id, .. } => Some(fork_run_id.as_str()),
        Event::AssistantSegmentComplete {
            fork_run_id: Some(id),
            ..
        } => Some(id.as_str()),
        _ => None,
    }
}

/// Send fork overlay stream/tool/segment events only when the UI subscribed to `run_id`.
pub fn try_send_fork_overlay_event(
    subs: &ForkStreamSubscriptions,
    tx: &mpsc::UnboundedSender<Event>,
    event: Event,
) {
    if let Some(run_id) = fork_overlay_run_id(&event) {
        if !is_fork_stream_subscribed(subs, run_id) {
            return;
        }
    }
    let _ = tx.send(event);
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::ContentBlockKind;

    #[test]
    fn gate_blocks_stream_when_not_subscribed() {
        let subs = new_fork_stream_subscriptions();
        let (tx, mut rx) = mpsc::unbounded_channel();
        try_send_fork_overlay_event(
            &subs,
            &tx,
            Event::SubAgentStreamDelta {
                fork_run_id: "fr-1".into(),
                delta: "hi".into(),
                kind: ContentBlockKind::Text,
            },
        );
        assert!(rx.try_recv().is_err());

        subs.write().unwrap().insert("fr-1".into());
        try_send_fork_overlay_event(
            &subs,
            &tx,
            Event::SubAgentStreamDelta {
                fork_run_id: "fr-1".into(),
                delta: "hi".into(),
                kind: ContentBlockKind::Text,
            },
        );
        assert!(rx.try_recv().is_ok());
    }

    #[test]
    fn started_not_gated_by_this_helper() {
        let (tx, mut rx) = mpsc::unbounded_channel();
        let _ = tx.send(Event::SubAgentStarted {
            fork_run_id: "fr-1".into(),
            agent_id: "a".into(),
            agent_type: "KnowledgeAuditor".into(),
            task_preview: "t".into(),
            parent_tool_call_id: None,
        });
        assert!(rx.try_recv().is_ok());
    }
}
