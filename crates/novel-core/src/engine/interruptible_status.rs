use super::types::AgentEngine;
use crate::Event;
use tokio::sync::mpsc;

impl AgentEngine {
    pub(crate) fn set_interruptible_tool_in_progress(
        &mut self,
        value: bool,
        event_tx: Option<&mpsc::UnboundedSender<Event>>,
    ) {
        if self.has_interruptible_tool_in_progress == value {
            return;
        }
        self.has_interruptible_tool_in_progress = value;
        tracing::debug!(has_interruptible = value, "interruptible_status_changed");
        if let Some(tx) = event_tx {
            let _ = tx.send(Event::InterruptibleStatusChanged {
                has_interruptible_tool_in_progress: value,
            });
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::engine::types::{AgentEngine, EngineConfig};
    use crate::Event;
    use tempfile::TempDir;

    fn test_config(tmp: &TempDir) -> EngineConfig {
        std::fs::create_dir_all(tmp.path().join("skills")).unwrap();
        EngineConfig {
            project_root: tmp.path().to_path_buf(),
            settings_path: tmp.path().join("settings.json"),
            db_path: tmp.path().join("state.db"),
            skills_dir: tmp.path().join("skills"),
            global_config_path: tmp.path().join(".novel-agent/api_config.json"),
        }
    }

    #[test]
    fn setter_skips_duplicate_emit() {
        let tmp = TempDir::new().unwrap();
        let mut engine = AgentEngine::new(test_config(&tmp)).unwrap();
        let (tx, mut rx) = mpsc::unbounded_channel();
        engine.set_interruptible_tool_in_progress(true, Some(&tx));
        engine.set_interruptible_tool_in_progress(true, Some(&tx));
        assert!(matches!(
            rx.try_recv().unwrap(),
            Event::InterruptibleStatusChanged {
                has_interruptible_tool_in_progress: true,
            }
        ));
        assert!(rx.try_recv().is_err());
    }

    #[test]
    fn setter_emits_on_toggle() {
        let tmp = TempDir::new().unwrap();
        let mut engine = AgentEngine::new(test_config(&tmp)).unwrap();
        let (tx, mut rx) = mpsc::unbounded_channel();
        engine.set_interruptible_tool_in_progress(true, Some(&tx));
        engine.set_interruptible_tool_in_progress(false, Some(&tx));
        assert!(matches!(
            rx.try_recv().unwrap(),
            Event::InterruptibleStatusChanged {
                has_interruptible_tool_in_progress: true,
            }
        ));
        assert!(matches!(
            rx.try_recv().unwrap(),
            Event::InterruptibleStatusChanged {
                has_interruptible_tool_in_progress: false,
            }
        ));
    }
}
