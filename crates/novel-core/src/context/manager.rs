use novel_compaction::CompactionStrategy;
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

    pub fn retain_policy(&self) -> &novel_compaction::RetainPolicy {
        &self.inner.retain
    }
}
