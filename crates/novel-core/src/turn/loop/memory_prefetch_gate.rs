//! Guards for whether turn-start memory prefetch should run.

use std::collections::HashSet;
use std::path::Path;

use novel_memory::{MemoryConstants, MemoryPrefetch};

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct MemoryPrefetchGate {
    pub word_count: usize,
    pub surfaced_bytes: usize,
    pub surfaced_paths: HashSet<String>,
    pub skip_short_prompt: bool,
    pub skip_budget_exceeded: bool,
}

impl MemoryPrefetchGate {
    pub(crate) fn should_skip(&self) -> bool {
        self.skip_short_prompt || self.skip_budget_exceeded
    }
}

pub(crate) fn evaluate_memory_prefetch_gate(
    author_content: &str,
    project_root: &Path,
    message_contents: &[&str],
) -> MemoryPrefetchGate {
    let word_count = novel_memory::estimated_word_count(author_content);
    let skip_short_prompt = word_count < MemoryConstants::MEMORY_PREFETCH_MIN_WORDS;
    let memory_dir = project_root.join("memory");
    let surfaced_paths = MemoryPrefetch::collect_surfaced_paths(message_contents.iter().copied());
    let surfaced_bytes = MemoryPrefetch::count_surfaced_bytes(&surfaced_paths, &memory_dir);
    let skip_budget_exceeded = surfaced_bytes >= MemoryConstants::MAX_SESSION_BYTES;
    MemoryPrefetchGate {
        word_count,
        surfaced_bytes,
        surfaced_paths,
        skip_short_prompt,
        skip_budget_exceeded,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn skips_very_short_author_prompt() {
        let tmp = TempDir::new().unwrap();
        let gate = evaluate_memory_prefetch_gate("好", tmp.path(), &[]);
        assert!(gate.skip_short_prompt);
        assert!(gate.should_skip());
    }

    #[test]
    fn allows_long_enough_prompt() {
        let tmp = TempDir::new().unwrap();
        let gate = evaluate_memory_prefetch_gate("写第5章正文", tmp.path(), &[]);
        assert!(!gate.skip_short_prompt);
    }
}
