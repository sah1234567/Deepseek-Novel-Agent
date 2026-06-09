//! Read-economy limits: reject oversized tool output before it enters LLM context.

use crate::{normalize_rel_path, ToolError, ToolOutput};
use serde_json::Value;

pub const KNOWLEDGE_MAX_LINES: usize = 80;
pub const CHAPTER_MAX_LINES: usize = 200;
pub const GREP_MAX_LINES: usize = 80;

pub fn count_lines(content: &str) -> usize {
    if content.is_empty() {
        0
    } else {
        content.lines().count()
    }
}

pub enum PathKind {
    Knowledge,
    Chapter,
    Other,
}

pub fn classify_path(path: &str) -> PathKind {
    let p = normalize_rel_path(path);
    if p.contains("knowledge/") || p.contains("memory/") || p.contains("plan/") {
        PathKind::Knowledge
    } else if p.contains("chapters/") || p.ends_with("AGENTS.md") {
        PathKind::Chapter
    } else {
        PathKind::Other
    }
}

pub fn max_lines_for_path(path: &str) -> Option<usize> {
    match classify_path(path) {
        PathKind::Knowledge => Some(KNOWLEDGE_MAX_LINES),
        PathKind::Chapter => Some(CHAPTER_MAX_LINES),
        PathKind::Other => None,
    }
}

pub fn read_pre_check(
    path: &str,
    limit: Option<usize>,
    total_lines: usize,
) -> Result<(), ToolError> {
    let Some(max) = max_lines_for_path(path) else {
        return Ok(());
    };
    if let Some(lim) = limit {
        if lim > max {
            return Err(ToolError::Execution(format!(
                "Read economy: limit {lim} exceeds max {max} for this path kind. Use offset+limit ≤ {max}."
            )));
        }
        return Ok(());
    }
    if total_lines > max {
        return Err(ToolError::Execution(format!(
            "Read economy: file has {total_lines} lines (max {max} without limit). \
             Use Grep to locate line numbers, then Read with offset+limit."
        )));
    }
    Ok(())
}

pub fn enforce_tool_output_limits(
    registry: &crate::ToolRegistry,
    tool_name: &str,
    tool_input: &Value,
    output: &ToolOutput,
) -> Result<ToolOutput, ToolError> {
    if output.is_error {
        return Ok(output.clone());
    }
    let Some(tool) = registry.get(tool_name) else {
        return Ok(output.clone());
    };
    let Some(max) = tool.max_output_lines(tool_input) else {
        return Ok(output.clone());
    };
    let lines = count_lines(&output.content);
    if lines > max {
        let hint = tool.output_limit_exceeded_hint();
        return Err(ToolError::Execution(format!(
            "Read economy: {tool_name} output has {lines} lines (max {max}). {hint}"
        )));
    }
    Ok(output.clone())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn knowledge_path_classified() {
        assert!(matches!(
            classify_path("knowledge/characters/x.md"),
            PathKind::Knowledge
        ));
    }

    #[test]
    fn read_pre_check_blocks_large_knowledge_file() {
        let err = read_pre_check("knowledge/plot/大纲.md", None, 116).unwrap_err();
        assert!(err.to_string().contains("116 lines"));
    }
}
