//! UTF-8 safe truncation (never slice `str` at arbitrary byte indices).

/// Truncate to at most `max_chars` Unicode scalars; append `suffix` when shortened.
/// The suffix char-count is subtracted from `max_chars` so the total output ≤ max_chars.
pub fn truncate_with_suffix(s: &str, max_chars: usize, suffix: &str) -> String {
    if s.chars().count() <= max_chars {
        return s.to_string();
    }
    let suffix_chars = suffix.chars().count();
    let take = max_chars.saturating_sub(suffix_chars).max(1);
    format!("{}{suffix}", s.chars().take(take).collect::<String>())
}

/// Truncate to at most `max_chars` Unicode scalars; append `…` when shortened.
pub fn truncate_chars(s: &str, max_chars: usize) -> String {
    truncate_with_suffix(s, max_chars, "\u{2026}")
}

/// Return the longest prefix of `s` whose UTF-8 byte length is ≤ `max_bytes`.
pub fn utf8_byte_prefix(s: &str, max_bytes: usize) -> String {
    if s.len() <= max_bytes {
        return s.to_string();
    }
    let mut end = max_bytes.min(s.len());
    while end > 0 && !s.is_char_boundary(end) {
        end -= 1;
    }
    s[..end].to_string()
}

/// Truncate to ≤ `max_bytes` UTF-8 bytes on a char boundary; append `…` when shortened.
pub fn truncate_bytes_utf8(s: &str, max_bytes: usize) -> String {
    if s.len() <= max_bytes {
        return s.to_string();
    }
    const ELLIPSIS: &str = "…";
    let ellipsis_len = ELLIPSIS.len();
    if max_bytes <= ellipsis_len {
        return ELLIPSIS.to_string();
    }
    let prefix = utf8_byte_prefix(s, max_bytes - ellipsis_len);
    format!("{prefix}{ELLIPSIS}")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn truncate_chars_long_chinese() {
        let s = "请".repeat(100);
        let out = truncate_chars(&s, 80);
        assert!(out.chars().count() <= 80);
        assert!(out.ends_with('…'));
    }

    #[test]
    fn utf8_byte_prefix_avoids_panic_on_cjk_boundary() {
        let s = "请".repeat(30); // 90 bytes
        let prefix = utf8_byte_prefix(&s, 80);
        assert!(prefix.len() <= 80);
        assert!(std::str::from_utf8(prefix.as_bytes()).is_ok());
    }

    #[test]
    fn truncate_bytes_utf8_on_cjk() {
        let s = format!("{}extra", "中".repeat(40));
        let out = truncate_bytes_utf8(&s, 80);
        assert!(out.len() <= 80);
        assert!(out.ends_with('…'));
    }
}
