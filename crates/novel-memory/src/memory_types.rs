//! Memory type system: 5 closed types, frontmatter schema, status tracking,
//! and shared body truncation.
//!
//! The 5-type classification is intentionally closed — new types require code changes
//! (open-closed principle by design — new types require code changes).

use serde::{Deserialize, Serialize};

/// 5 closed memory types. Deprecated is NOT a type — it's a status.
pub const MEMORY_TYPES: &[&str] = &[
    "style",
    "plot_decision",
    "character_guardrail",
    "feedback",
    "reference",
];

#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum MemoryType {
    /// Writing style preferences (pacing, tone, description habits)
    Style,
    /// Irreversible plot decisions
    PlotDecision,
    /// Character guardrails (things a character must never do)
    CharacterGuardrail,
    /// External feedback and confirmed patterns
    Feedback,
    /// External references, inspirations, benchmark works
    Reference,
}

impl MemoryType {
    /// English string used in memory manifests (machine-readable).
    pub fn as_str(&self) -> &str {
        match self {
            MemoryType::Style => "style",
            MemoryType::PlotDecision => "plot_decision",
            MemoryType::CharacterGuardrail => "character_guardrail",
            MemoryType::Feedback => "feedback",
            MemoryType::Reference => "reference",
        }
    }

    /// Chinese label used in system prompt display.
    pub fn label(&self) -> &str {
        match self {
            MemoryType::Style => "文风",
            MemoryType::PlotDecision => "剧情决策",
            MemoryType::CharacterGuardrail => "人物禁区",
            MemoryType::Feedback => "反馈",
            MemoryType::Reference => "参考",
        }
    }

    /// Derive the memory type from the first path component of a relative path.
    /// E.g. `"style/pacing.md"` → `MemoryType::Style`.
    pub fn from_rel_path(rel: &str) -> Option<MemoryType> {
        let dir = rel.split('/').next()?;
        match dir {
            "style" => Some(MemoryType::Style),
            "plot_decisions" => Some(MemoryType::PlotDecision),
            "character_guardrails" => Some(MemoryType::CharacterGuardrail),
            "feedback" => Some(MemoryType::Feedback),
            "references" => Some(MemoryType::Reference),
            _ => None,
        }
    }
}

/// Memory lifecycle status.
///
/// - Active → Superseded: replaced by a new decision (original kept, marked as superseded)
/// - Active → Deprecated: no longer applicable (original kept — prevents agent from re-proposing)
///
/// Deprecating a memory means editing the original file's `status` field — NOT creating a new file.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "lowercase")]
pub enum MemoryStatus {
    Active,
    Superseded,
    Deprecated,
}

impl MemoryStatus {
    /// Whether this status represents an active (non-archived) memory.
    /// Used by scanning and selection to filter out deprecated/superseded files.
    pub fn is_active(&self) -> bool {
        matches!(self, MemoryStatus::Active)
    }
}

/// Mandatory frontmatter for every `memory/` *.md file.
///
/// Type is derived from the parent directory (e.g. `style/pacing.md` → Style),
/// not stored in the YAML frontmatter.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct MemoryFrontmatter {
    /// Short kebab-case slug (becomes the filename stem)
    pub name: String,
    /// One-line summary — used during relevance matching
    pub description: String,
    /// Chapter when this memory was recorded (e.g. "Ch12" or "global")
    pub chapter: String,
    /// active | superseded | deprecated
    #[serde(default = "default_status")]
    pub status: MemoryStatus,
}

fn default_status() -> MemoryStatus {
    MemoryStatus::Active
}

/// Memory system constants — single source of truth for all limits.
///
/// Defined here so that `memory_scan`, `memory_select`, and `prefetch` all
/// reference the same values without duplicating magic numbers.
pub struct MemoryConstants;

impl MemoryConstants {
    /// Max lines to read when scanning frontmatter only.
    /// 30 lines is enough for any YAML frontmatter block.
    pub const FRONTMATTER_MAX_LINES: usize = 30;
    /// Max memory files to scan (mtime-ordered, newest first).
    pub const MAX_MEMORY_FILES: usize = 200;
    /// Max lines of a single memory body injected into context.
    pub const MAX_MEMORY_LINES: usize = 200;
    /// Max bytes of a single memory body injected into context.
    pub const MAX_MEMORY_BYTES: usize = 4096;
    /// Cumulative memory bytes per session.
    pub const MAX_SESSION_BYTES: usize = 60 * 1024;
    /// Minimum "word count" in a user message to trigger memory prefetch.
    /// Uses a mixed CJK/ASCII heuristic: each CJK character = 1 word,
    /// ASCII runs count as whitespace-separated tokens.  Effective examples:
    /// "好"(1) → skip, "继续写"(3) → run, "write ch 5"(3) → run.
    pub const MEMORY_PREFETCH_MIN_WORDS: usize = 4;
    /// Turns between memory extraction runs.  Default 1 (every eligible turn).
    /// Increase to reduce extraction cost when turns are rapid and content-light.
    pub const EXTRACTION_THROTTLE_TURNS: u32 = 1;
    /// Output token limit for the Flash memory selector.
    /// 256 tokens ≈ 5 filenames × 50 chars. Output exceeding this likely
    /// means the model is emitting explanations instead of filenames.
    pub const FLASH_MAX_TOKENS: u32 = 256;
}

/// Combine CJK character count (each ≈ one semantic unit) and ASCII
/// whitespace-split word count.  Returns the larger of the two so
/// short prompts in either language are reliably detected.
pub fn estimated_word_count(text: &str) -> usize {
    let trimmed = text.trim();
    if trimmed.is_empty() {
        return 0;
    }
    let cjk_chars = trimmed
        .chars()
        .filter(|c| {
            matches!(
                c,
                '\u{4E00}'..='\u{9FFF}'
                    | '\u{3400}'..='\u{4DBF}'
                    | '\u{F900}'..='\u{FAFF}'
                    | '\u{3000}'..='\u{303F}'
                    | '\u{FF00}'..='\u{FFEF}'
            )
        })
        .count();
    let ascii_words = trimmed.split_whitespace().count();
    cjk_chars.max(ascii_words)
}

/// A memory file header (frontmatter + derived type). Returned by `scan_memory_files`.
#[derive(Debug, Clone)]
pub struct MemoryHeader {
    /// File path relative to memory/ dir (e.g. "style/pacing.md")
    pub rel_path: String,
    /// Memory type derived from the parent directory (e.g. `style/` → Style)
    pub memory_type: MemoryType,
    /// Parsed frontmatter
    pub frontmatter: MemoryFrontmatter,
    /// File modification time (Unix ms) — used for mtime ordering
    pub mtime_ms: u64,
}

/// A fully loaded memory ready for injection into context.
#[derive(Debug, Clone)]
pub struct SurfacedMemory {
    pub header: MemoryHeader,
    /// Full body content (≤ MAX_MEMORY_BYTES)
    pub content: String,
    /// Whether content was truncated (exceeded MAX_MEMORY_LINES or MAX_MEMORY_BYTES)
    pub truncated: bool,
}

/// Result of a lightweight side query via the unified streaming path.
#[derive(Debug, Clone)]
pub struct SideQueryResult {
    pub content: String,
}

/// Chinese description for a memory type key — used when building
/// table headers in memory manifests (data-driven from `MEMORY_TYPES`).
pub(crate) fn memory_type_description(type_name: &str) -> &str {
    match type_name {
        "style" => "文风偏好、节奏、描写习惯",
        "plot_decision" => "不可逆剧情决策与理由",
        "character_guardrail" => "人物塑造禁区",
        "feedback" => "外部反馈、读者意见、确认的模式",
        "reference" => "外部参考、灵感来源、对标作品",
        _ => "",
    }
}

/// Format a memory header with staleness distance.
///
/// When the memory was recorded more than 20 chapters ago, adds a staleness
/// warning so the agent can judge whether the memory is still applicable.
///
/// Example: `Memory (记录于 Ch5，距当前 25 章): style/pacing.md:`
pub fn memory_header(path: &str, chapter: &str, current_chapter: u32) -> String {
    let ch_num = parse_chapter_number(chapter);
    let distance = current_chapter.saturating_sub(ch_num);
    if distance > 20 {
        format!("Memory (记录于 {chapter}，距当前 {distance} 章): {path}:")
    } else {
        format!("Memory (记录于 {chapter}): {path}:")
    }
}

/// Extract the chapter number from a "ChN" string.
///
/// Returns `u32::MAX` for non-numeric values (e.g. `"global"`) so that
/// `current_chapter.saturating_sub(u32::MAX)` is always 0 — global memories
/// never trigger staleness warnings.
fn parse_chapter_number(chapter: &str) -> u32 {
    chapter
        .strip_prefix("Ch")
        .or_else(|| chapter.strip_prefix("ch"))
        .and_then(|s| s.parse::<u32>().ok())
        .unwrap_or(u32::MAX)
}

/// Truncate a string to at most `max_bytes` UTF-8 bytes, appending `…` when
/// shortened. Always returns a valid UTF-8 string — never slices mid-codepoint.
///
/// Inlined from `novel-knowledge::text_util` to remove the crate dependency.
pub(crate) fn truncate_bytes_utf8(s: &str, max_bytes: usize) -> String {
    if s.len() <= max_bytes {
        return s.to_string();
    }
    const ELLIPSIS: &str = "…";
    if max_bytes <= ELLIPSIS.len() {
        return ELLIPSIS.to_string();
    }
    let mut end = (max_bytes - ELLIPSIS.len()).min(s.len());
    while end > 0 && !s.is_char_boundary(end) {
        end -= 1;
    }
    format!("{}{ELLIPSIS}", &s[..end])
}

/// Apply line-count and byte-size caps per [`MemoryConstants`].
///
/// Returns the truncated body and a flag indicating whether truncation occurred.
/// Shared by `loading`, `prefetch`, and selection pipelines.
pub(crate) fn truncate_memory_body(content: &str) -> (String, bool) {
    let lines: Vec<&str> = content.lines().collect();
    let line_truncated = lines.len() > MemoryConstants::MAX_MEMORY_LINES;
    let body: String = lines
        .into_iter()
        .take(MemoryConstants::MAX_MEMORY_LINES)
        .collect::<Vec<_>>()
        .join("\n");
    let byte_truncated = body.len() > MemoryConstants::MAX_MEMORY_BYTES;
    let final_content = if byte_truncated {
        truncate_bytes_utf8(&body, MemoryConstants::MAX_MEMORY_BYTES)
    } else {
        body
    };
    (final_content, line_truncated || byte_truncated)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn constants_are_nonzero() {
        const {
            assert!(MemoryConstants::FRONTMATTER_MAX_LINES > 0);
            assert!(MemoryConstants::MAX_MEMORY_FILES > 0);
            assert!(MemoryConstants::MAX_MEMORY_BYTES > 0);
            assert!(MemoryConstants::FLASH_MAX_TOKENS > 0);
        }
    }

    #[test]
    fn memory_header_no_staleness_warning_when_close() {
        let h = memory_header("style/pacing.md", "Ch5", 10);
        assert!(h.contains("Ch5"));
        assert!(!h.contains("距当前"));
    }

    #[test]
    fn memory_header_adds_staleness_warning_after_20_chapters() {
        let h = memory_header("plot_decisions/cp.md", "Ch1", 30);
        assert!(h.contains("Ch1"));
        assert!(h.contains("距当前 29 章"));
    }

    #[test]
    fn parse_chapter_number_handles_ch_prefix() {
        assert_eq!(parse_chapter_number("Ch31"), 31);
        assert_eq!(parse_chapter_number("global"), u32::MAX);
    }
}
