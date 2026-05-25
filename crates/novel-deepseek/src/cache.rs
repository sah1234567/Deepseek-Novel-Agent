use crate::TokenUsage;

#[derive(Debug, Clone, Default)]
pub struct CacheStats {
    pub total_requests: u64,
    pub total_hit_tokens: i64,
    pub total_miss_tokens: i64,
    pub total_completion_tokens: i64,
}

#[derive(Debug, Clone, Default)]
pub struct CacheTracker {
    stats: CacheStats,
}

impl CacheTracker {
    pub fn record(&mut self, usage: &TokenUsage) {
        self.stats.total_requests += 1;
        self.stats.total_hit_tokens += usage.cache_hit_tokens;
        self.stats.total_miss_tokens += usage.cache_miss_tokens;
        self.stats.total_completion_tokens += usage.completion_tokens;
    }

    pub fn stats(&self) -> &CacheStats {
        &self.stats
    }

    pub fn hit_rate(&self) -> f64 {
        let denom = self.stats.total_hit_tokens + self.stats.total_miss_tokens;
        if denom == 0 {
            0.0
        } else {
            self.stats.total_hit_tokens as f64 / denom as f64
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn record_and_hit_rate() {
        let mut t = CacheTracker::default();
        t.record(&TokenUsage::from_deepseek_usage(50, 50, 10, 0));
        assert_eq!(t.stats().total_requests, 1);
        assert!((t.hit_rate() - 0.5).abs() < f64::EPSILON);
    }
}
