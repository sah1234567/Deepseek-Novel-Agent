use novel_compaction::{CompactionDecision, CompactionStrategy};
use novel_config::ModelConfig;

#[derive(Clone)]
pub struct ContextManager {
    pub(crate) inner: CompactionStrategy,
}

impl ContextManager {
    pub fn new(model: &ModelConfig) -> Self {
        Self {
            inner: CompactionStrategy::from_model(model),
        }
    }

    pub fn window_size(&self) -> usize {
        self.inner.window_size
    }

    pub fn threshold(&self) -> f32 {
        self.inner.threshold
    }

    pub fn check_budget(&self, messages: &[&str]) -> CompactionDecision {
        self.inner.evaluate(messages)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn context_manager_no_panic_empty() {
        let cm = ContextManager::new(&ModelConfig::default());
        let _ = cm.check_budget(&[]);
    }
}
