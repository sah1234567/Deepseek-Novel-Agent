use crate::{estimate_tokens, RetainPolicy};
use novel_config::ModelConfig;

#[derive(Debug, Clone, PartialEq)]
pub enum CompactionDecision {
    NoAction {
        usage_ratio: f32,
    },
    ShouldCompact {
        usage_ratio: f32,
        current_tokens: usize,
        window: usize,
    },
}

#[derive(Clone)]
pub struct CompactionStrategy {
    pub window_size: usize,
    pub threshold: f32,
    pub target_ratio: f32,
    pub retain: RetainPolicy,
}

impl CompactionStrategy {
    pub fn from_model(model: &ModelConfig) -> Self {
        Self {
            window_size: model.context_window_size,
            threshold: model.compaction_threshold,
            target_ratio: 0.5,
            retain: RetainPolicy::default(),
        }
    }

    pub fn evaluate(&self, messages: &[&str]) -> CompactionDecision {
        let current: usize = messages.iter().map(|m| estimate_tokens(m)).sum();
        let usage = current as f32 / self.window_size as f32;
        if usage >= self.threshold {
            CompactionDecision::ShouldCompact {
                usage_ratio: usage,
                current_tokens: current,
                window: self.window_size,
            }
        } else {
            CompactionDecision::NoAction { usage_ratio: usage }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rstest::rstest;

    #[rstest]
    #[test]
    fn no_compact_below_threshold() {
        let s = CompactionStrategy::from_model(&ModelConfig::default());
        let msgs = vec!["hello"; 10];
        let refs: Vec<&str> = msgs.iter().map(|s| *s).collect();
        assert!(matches!(
            s.evaluate(&refs),
            CompactionDecision::NoAction { .. }
        ));
    }

    #[rstest]
    #[test]
    fn compact_when_over_threshold() {
        let mut model = ModelConfig::default();
        model.context_window_size = 10;
        model.compaction_threshold = 0.5;
        let s = CompactionStrategy::from_model(&model);
        let big = "x".repeat(100);
        let refs = vec![big.as_str()];
        assert!(matches!(
            s.evaluate(&refs),
            CompactionDecision::ShouldCompact { .. }
        ));
    }
}
