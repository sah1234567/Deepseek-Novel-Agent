//! Memory relevance selection: prompt constants, response parsing,
//! and file reading for surfacing memories into context.
//!
//! The LLM side-query orchestration lives in [`super::selection`].

use crate::memory_types::{truncate_memory_body, MemoryHeader, SurfacedMemory};
use serde::Deserialize;
use std::path::Path;

/// System prompt for the memory selector (Flash side-query).
pub const SELECT_MEMORIES_SYSTEM_PROMPT: &str = r#"你正在为小说写作 Agent 选择对当前写作任务有用的记忆。你会收到当前任务，以及一份可用记忆文件的清单（含子目录分类和摘要，例如 `[style] pacing.md: 节奏偏好`）。

返回明确有用的记忆文件名列表（最多 5 个）。仅包含你根据分类和摘要确信有帮助的记忆。
- 优先选择分类与当前任务匹配的记忆（例如写作任务选 style/，剧情规划选 plot_decisions/）。
- 如果不确定某条记忆是否有用，不要选它。保持挑剔和精准。
- 如果没有明确有用的记忆，返回空列表。
- 不要选择标记为 'deprecated' 或 'superseded' 的记忆。
"#;

/// JSON schema for the selector response.
pub const SELECT_MEMORIES_SCHEMA: &str = r#"{
  "type": "json_schema",
  "json_schema": {
    "name": "selected_memories",
    "strict": true,
    "schema": {
      "type": "object",
      "properties": {
        "selected_memories": {
          "type": "array",
          "items": { "type": "string" },
          "maxItems": 5,
          "description": "与当前任务相关的记忆文件名（路径）列表"
        }
      },
      "required": ["selected_memories"],
      "additionalProperties": false
    }
  }
}"#;

#[derive(Debug, Deserialize)]
pub struct SelectorResponse {
    pub selected_memories: Vec<String>,
}

/// Parse the Flash selector's JSON response into a list of filenames.
///
/// Tries strict JSON first, then embedded JSON extraction, then
/// line-by-line fallback for resilience against model formatting quirks.
pub fn parse_selector_response(content: &str) -> Vec<String> {
    // Try parsing as JSON first
    if let Ok(resp) = serde_json::from_str::<SelectorResponse>(content) {
        return resp.selected_memories;
    }
    // Fallback: try to extract from JSON embedded in text
    if let Some(json) = extract_embedded_json(content) {
        if let Ok(resp) = serde_json::from_str::<SelectorResponse>(&json) {
            return resp.selected_memories;
        }
    }
    // Last resort: parse line-by-line looking for filenames
    parse_selector_lines(content)
}

fn extract_embedded_json(content: &str) -> Option<String> {
    let start = content.find('{')?;
    let end = content.rfind('}')?;
    Some(content[start..=end].to_string())
}

fn parse_selector_lines(content: &str) -> Vec<String> {
    content
        .lines()
        .filter_map(|line| {
            let trimmed = line.trim();
            if trimmed.is_empty() || trimmed.starts_with('[') || trimmed.starts_with('{') {
                return None;
            }
            let cleaned =
                trimmed.trim_matches(|c: char| c == '"' || c == ',' || c == '[' || c == ']');
            cleaned.ends_with(".md").then(|| cleaned.to_string())
        })
        .take(5)
        .collect()
}

/// Read the full body of selected memory files for surfacing into context.
///
/// Each file is capped at `MAX_MEMORY_BYTES` bytes and `MAX_MEMORY_LINES` lines.
pub fn read_memories_for_surfacing(
    memory_dir: &Path,
    headers: &[MemoryHeader],
    selected_filenames: &[String],
) -> Vec<SurfacedMemory> {
    selected_filenames
        .iter()
        .filter_map(|filename| {
            let header = headers.iter().find(|h| h.rel_path == *filename)?.clone();
            let content = std::fs::read_to_string(memory_dir.join(filename)).ok()?;
            let (body, truncated) = truncate_memory_body(&content);
            Some(SurfacedMemory {
                header,
                content: body,
                truncated,
            })
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::memory_types::{MemoryConstants, MemoryFrontmatter, MemoryStatus, MemoryType};
    use std::fs;
    use tempfile::TempDir;

    #[test]
    fn parse_selector_response_valid_json() {
        let json = r#"{"selected_memories": ["style/pacing.md", "plot_decisions/cp.md"]}"#;
        let result = parse_selector_response(json);
        assert_eq!(result, vec!["style/pacing.md", "plot_decisions/cp.md"]);
    }

    #[test]
    fn parse_selector_response_empty() {
        let json = r#"{"selected_memories": []}"#;
        let result = parse_selector_response(json);
        assert!(result.is_empty());
    }

    #[test]
    fn parse_selector_response_fallback() {
        let text = r#"Based on the task, here are the relevant memories:
{"selected_memories": ["style/pacing.md"]}"#;
        let result = parse_selector_response(text);
        assert_eq!(result, vec!["style/pacing.md"]);
    }

    #[test]
    fn parse_selector_response_line_fallback() {
        let text = "style/pacing.md\nplot_decisions/cp.md";
        let result = parse_selector_response(text);
        assert_eq!(result.len(), 2);
        assert!(result.contains(&"style/pacing.md".to_string()));
    }

    // ── read_memories_for_surfacing ──

    fn make_header(rel: &str) -> MemoryHeader {
        MemoryHeader {
            rel_path: rel.into(),
            memory_type: MemoryType::Style,
            frontmatter: MemoryFrontmatter {
                name: "test".into(),
                description: "test".into(),
                chapter: "Ch1".into(),
                status: MemoryStatus::Active,
            },
            mtime_ms: 0,
        }
    }

    #[test]
    fn read_memories_returns_content() {
        let tmp = TempDir::new().unwrap();
        let file_path = "style/pacing.md";
        let full_path = tmp.path().join(file_path);
        fs::create_dir_all(full_path.parent().unwrap()).unwrap();
        fs::write(&full_path, "## Test\n\nBody content here.\n").unwrap();

        let headers = vec![make_header(file_path)];
        let selected = vec![file_path.to_string()];
        let results = read_memories_for_surfacing(tmp.path(), &headers, &selected);
        assert_eq!(results.len(), 1);
        assert!(!results[0].truncated);
        assert!(results[0].content.contains("Body content"));
    }

    #[test]
    fn read_memories_skips_missing_file() {
        let tmp = TempDir::new().unwrap();
        let headers = vec![make_header("nonexistent.md")];
        let selected = vec!["nonexistent.md".to_string()];
        let results = read_memories_for_surfacing(tmp.path(), &headers, &selected);
        assert!(results.is_empty());
    }

    #[test]
    fn read_memories_truncates_long_content() {
        let tmp = TempDir::new().unwrap();
        let file_path = "style/long.md";
        let full_path = tmp.path().join(file_path);
        fs::create_dir_all(full_path.parent().unwrap()).unwrap();
        let long_body = "长".repeat(5000);
        fs::write(&full_path, &long_body).unwrap();

        let headers = vec![make_header(file_path)];
        let selected = vec![file_path.to_string()];
        let results = read_memories_for_surfacing(tmp.path(), &headers, &selected);
        assert_eq!(results.len(), 1);
        assert!(results[0].truncated);
        assert!(results[0].content.len() <= MemoryConstants::MAX_MEMORY_BYTES + 10);
    }

    #[test]
    fn read_memories_truncates_many_lines() {
        let tmp = TempDir::new().unwrap();
        let file_path = "style/many_lines.md";
        let full_path = tmp.path().join(file_path);
        fs::create_dir_all(full_path.parent().unwrap()).unwrap();
        let many_lines: String = (0..300).map(|i| format!("line {i}\n")).collect();
        fs::write(&full_path, &many_lines).unwrap();

        let headers = vec![make_header(file_path)];
        let selected = vec![file_path.to_string()];
        let results = read_memories_for_surfacing(tmp.path(), &headers, &selected);
        assert_eq!(results.len(), 1);
        assert!(results[0].truncated);
    }

    #[test]
    fn read_memories_filters_by_selected() {
        let tmp = TempDir::new().unwrap();
        let style_path = "style/keep.md";
        let plot_path = "plot_decisions/skip.md";
        fs::create_dir_all(tmp.path().join("style")).unwrap();
        fs::create_dir_all(tmp.path().join("plot_decisions")).unwrap();
        fs::write(tmp.path().join(style_path), "keep content").unwrap();
        fs::write(tmp.path().join(plot_path), "skip content").unwrap();

        let headers = vec![make_header(style_path)];
        let selected = vec![style_path.to_string()];
        let results = read_memories_for_surfacing(tmp.path(), &headers, &selected);
        assert_eq!(results.len(), 1);
        assert!(results[0].content.contains("keep content"));
    }
}
