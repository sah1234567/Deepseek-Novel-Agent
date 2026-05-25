/// Estimate token count from text length.
/// Simple char-based fallback — real compaction decisions are driven by
/// the API response's cache_hit + cache_miss + completion token counts in novel-core.
/// This is only used by the offline compaction pipeline as a rough heuristic.
pub fn estimate_tokens(text: &str) -> usize {
    if text.is_empty() {
        return 0;
    }
    // Chinese text: ~1 token per 2 chars
    text.chars().count() / 2 + 1
}
