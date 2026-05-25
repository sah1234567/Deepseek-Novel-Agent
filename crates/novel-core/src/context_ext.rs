use crate::context::ContextManager;
use novel_compaction::CompactionStrategy;

impl ContextManager {
    pub fn retain_policy(&self) -> &novel_compaction::RetainPolicy {
        &self.strategy().retain
    }

    pub fn strategy(&self) -> &CompactionStrategy {
        &self.inner
    }
}
