//! Author-facing interaction mode: orchestrate graph vs work inside a focused node.

use serde::{Deserialize, Serialize};

/// Chat interaction mode (orthogonal to [`novel_tools::PermissionMode`]).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum InteractionMode {
    /// Build / schedule the plan graph (Interview or Orchestrator tools).
    #[default]
    Orchestrate,
    /// Execute the focused node (NodeExecution tools + node-execution prompt).
    Work,
}

impl InteractionMode {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Orchestrate => "orchestrate",
            Self::Work => "work",
        }
    }

    /// Strict parse for IPC (`set_interaction_mode`).
    pub fn parse(s: &str) -> Result<Self, String> {
        match s.trim().to_ascii_lowercase().as_str() {
            "orchestrate" | "orchestration" | "graph" => Ok(Self::Orchestrate),
            "work" | "interact" | "node" => Ok(Self::Work),
            other => Err(format!(
                "unknown interaction mode '{other}' — use orchestrate or work"
            )),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_aliases() {
        assert_eq!(
            InteractionMode::parse("orchestrate").unwrap(),
            InteractionMode::Orchestrate
        );
        assert_eq!(
            InteractionMode::parse("WORK").unwrap(),
            InteractionMode::Work
        );
        assert!(InteractionMode::parse("nope").is_err());
    }
}
