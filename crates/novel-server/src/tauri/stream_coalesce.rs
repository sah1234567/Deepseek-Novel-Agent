//! Merge high-frequency stream deltas before Tauri emit (50ms window).

use novel_core::{ContentBlockKind, Event};
use std::collections::HashMap;
use std::time::Duration;
use tauri::AppHandle;

use super::events::emit_core_event;

const COALESCE_WINDOW: Duration = Duration::from_millis(50);

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
struct MainKey {
    message_id: String,
    kind: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
struct ForkKey {
    fork_run_id: String,
    kind: String,
}

fn kind_key(kind: ContentBlockKind) -> String {
    format!("{kind:?}").to_lowercase()
}

pub struct StreamCoalescer {
    main: HashMap<MainKey, Event>,
    fork: HashMap<ForkKey, Event>,
}

impl StreamCoalescer {
    pub fn new() -> Self {
        Self {
            main: HashMap::new(),
            fork: HashMap::new(),
        }
    }

    pub fn coalesce_window() -> Duration {
        COALESCE_WINDOW
    }

    /// Non-coalescable events must flush buffered stream deltas first.
    pub fn should_flush_before(event: &Event) -> bool {
        !matches!(
            event,
            Event::ContentBlockDelta { .. } | Event::SubAgentStreamDelta { .. }
        )
    }

    /// Buffer coalescable stream deltas. Returns `None` when buffered; otherwise the event to emit.
    pub fn try_buffer(&mut self, event: Event) -> Option<Event> {
        match event {
            Event::ContentBlockDelta {
                message_id,
                index,
                delta,
                kind,
            } => {
                let key = MainKey {
                    message_id: message_id.clone(),
                    kind: kind_key(kind),
                };
                self.main
                    .entry(key)
                    .and_modify(|e| {
                        if let Event::ContentBlockDelta { delta: d, .. } = e {
                            d.push_str(&delta);
                        }
                    })
                    .or_insert(Event::ContentBlockDelta {
                        message_id,
                        index,
                        delta,
                        kind,
                    });
                None
            }
            Event::SubAgentStreamDelta {
                fork_run_id,
                delta,
                kind,
            } => {
                let key = ForkKey {
                    fork_run_id: fork_run_id.clone(),
                    kind: kind_key(kind),
                };
                self.fork
                    .entry(key)
                    .and_modify(|e| {
                        if let Event::SubAgentStreamDelta { delta: d, .. } = e {
                            d.push_str(&delta);
                        }
                    })
                    .or_insert(Event::SubAgentStreamDelta {
                        fork_run_id,
                        delta,
                        kind,
                    });
                None
            }
            other => Some(other),
        }
    }

    pub fn flush_all(&mut self, app: &AppHandle, message_id: &str) {
        let main: Vec<Event> = self.main.drain().map(|(_, e)| e).collect();
        let fork: Vec<Event> = self.fork.drain().map(|(_, e)| e).collect();
        for event in main.into_iter().chain(fork) {
            emit_core_event(app, event, message_id);
        }
    }
}

impl Default for StreamCoalescer {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn merges_main_deltas_by_kind() {
        let mut c = StreamCoalescer::new();
        assert!(c
            .try_buffer(Event::ContentBlockDelta {
                message_id: "m1".into(),
                index: 0,
                delta: "a".into(),
                kind: ContentBlockKind::Text,
            })
            .is_none());
        assert!(c
            .try_buffer(Event::ContentBlockDelta {
                message_id: "m1".into(),
                index: 0,
                delta: "b".into(),
                kind: ContentBlockKind::Text,
            })
            .is_none());
        assert_eq!(c.main.len(), 1);
        if let Event::ContentBlockDelta { delta, .. } = c.main.values().next().unwrap() {
            assert_eq!(delta, "ab");
        } else {
            panic!("expected merged delta");
        }
    }

    #[test]
    fn flush_before_segment_complete() {
        assert!(StreamCoalescer::should_flush_before(
            &Event::AssistantSegmentComplete {
                segment_index: 1,
                fork_run_id: None,
            }
        ));
        assert!(!StreamCoalescer::should_flush_before(
            &Event::ContentBlockDelta {
                message_id: "m".into(),
                index: 0,
                delta: "x".into(),
                kind: ContentBlockKind::Thinking,
            }
        ));
    }

    #[test]
    fn flush_before_tool_events() {
        assert!(StreamCoalescer::should_flush_before(
            &Event::ToolCallRequest {
                tool_call_id: "t".into(),
                name: "Read".into(),
                input: serde_json::json!({}),
                needs_approval: false,
            }
        ));
        assert!(StreamCoalescer::should_flush_before(
            &Event::ToolCallResult {
                tool_call_id: "t".into(),
                content: "ok".into(),
            }
        ));
    }
}
