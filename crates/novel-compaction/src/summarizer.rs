const COMPACTION_SUMMARY_TRAILING: &str =
    include_str!("../../../prompt/compaction-summary-trailing.md");

/// Trailing user message appended after the cached session prefix for KV-cache-aware summarization.
pub fn build_summary_trailing_user_prompt() -> String {
    COMPACTION_SUMMARY_TRAILING.trim().to_string()
}

/// Truncate summary text to max_chars (character count).
pub fn truncate_summary(summary: &str, max_chars: usize) -> String {
    novel_knowledge::truncate_with_suffix(summary, max_chars, "\u{2026}")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn trailing_prompt_has_marker_and_no_middle_placeholder() {
        let p = build_summary_trailing_user_prompt();
        assert!(p.contains("[压缩摘要请求]"));
        assert!(!p.contains("{middle_text}"));
        assert!(p.contains("10000字左右"));
        assert!(!p.contains("硬上限"));
        assert!(p.contains("## 创作进度"));
        assert!(p.contains("## 审计状态"));
    }

    #[test]
    fn truncate_summary_respects_limit() {
        let s = "字".repeat(20);
        let out = truncate_summary(&s, 10);
        assert!(out.chars().count() <= 10);
    }
}
