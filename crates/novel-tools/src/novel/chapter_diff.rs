//! ChapterDiff — read-only line-level diff summary between two chapter versions.
//!
//! Solves the REGATE "compare against the previous draft" step: the agent currently
//! compares drafts by hand. Pure function, no external diff crate: common-prefix/suffix
//! stripping + middle-section line-set comparison.

use crate::{require_str, Tool, ToolContext, ToolError, ToolOutput};
use async_trait::async_trait;
use serde_json::{json, Value};
use std::collections::HashMap;
use std::path::Path;

pub struct ChapterDiffTool;

/// (added, removed, unchanged) counts plus change region (old line range).
fn diff_lines(old: &str, new: &str) -> (usize, usize, usize, Option<(usize, usize)>) {
    let a: Vec<&str> = old.lines().collect();
    let b: Vec<&str> = new.lines().collect();

    // Common prefix / suffix.
    let mut pre = 0usize;
    while pre < a.len() && pre < b.len() && a[pre] == b[pre] {
        pre += 1;
    }
    let mut suf = 0usize;
    while suf < a.len().saturating_sub(pre)
        && suf < b.len().saturating_sub(pre)
        && a[a.len() - 1 - suf] == b[b.len() - 1 - suf]
    {
        suf += 1;
    }

    let a_mid = &a[pre..a.len().saturating_sub(suf)];
    let b_mid = &b[pre..b.len().saturating_sub(suf)];

    // Line-set comparison on the middle section (counts, not alignment).
    let mut a_counts: HashMap<&str, usize> = HashMap::new();
    for line in a_mid {
        *a_counts.entry(line).or_insert(0) += 1;
    }
    let mut removed = 0usize;
    let mut added = 0usize;
    let mut b_seen: HashMap<&str, usize> = HashMap::new();
    for line in b_mid {
        let seen = b_seen.entry(line).or_insert(0);
        let avail = a_counts.get(line).copied().unwrap_or(0);
        if *seen < avail {
            *seen += 1; // matched
        } else {
            added += 1;
        }
    }
    for (line, count) in &a_counts {
        let matched = b_seen.get(line).copied().unwrap_or(0);
        removed += count.saturating_sub(matched);
    }

    let region = if a_mid.is_empty() && b_mid.is_empty() {
        None
    } else {
        Some((pre + 1, a.len().saturating_sub(suf))) // 1-based old-side range
    };
    (added, removed, pre + suf, region)
}

fn tail_changed(old: &str, new: &str) -> bool {
    let a: Vec<&str> = old.lines().collect();
    let b: Vec<&str> = new.lines().collect();
    a.last() != b.last()
}

fn build_summary(old_path: &str, new_path: &str, old: &str, new: &str) -> String {
    let (added, removed, unchanged, region) = diff_lines(old, new);
    let old_lines = old.lines().count();
    let new_lines = new.lines().count();
    let delta = new_lines as i64 - old_lines as i64;

    let mut lines = vec![format!("## ChapterDiff ({old_path} → {new_path})")];
    lines.push(format!(
        "- 行数: {old_lines} → {new_lines} ({delta:+}) | 新增 {added} 行, 删除 {removed} 行, 相同 {unchanged} 行"
    ));
    match region {
        Some((start, end)) => {
            if start <= end {
                lines.push(format!("- 变化区域(旧版行号): 行 {start}-{end}"));
            }
        }
        None => lines.push("- 内容未变化".to_string()),
    }
    if tail_changed(old, new) {
        let old_tail = old.lines().last().unwrap_or("").trim();
        let new_tail = new.lines().last().unwrap_or("").trim();
        lines.push("- 末行已变化:".to_string());
        lines.push(format!("  - 旧: {}", truncate(old_tail, 60)));
        lines.push(format!("  - 新: {}", truncate(new_tail, 60)));
    }
    lines.join("\n")
}

fn truncate(s: &str, max: usize) -> String {
    let mut out: String = s.chars().take(max).collect();
    if s.chars().count() > max {
        out.push('…');
    }
    out
}

#[async_trait]
impl Tool for ChapterDiffTool {
    fn name(&self) -> &str {
        "ChapterDiff"
    }
    fn description(&self) -> &str {
        "Line-level diff summary between two versions of a chapter file (added/removed/unchanged counts, changed region, tail change) — for REGATE before/after comparison"
    }
    fn input_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "path1": {"type": "string", "description": "Old version path (relative to work root), e.g. chapters/chapter-012.md"},
                "path2": {"type": "string", "description": "New version path, e.g. chapters/chapter-012.md or knowledge/meta/versions/chapter-012-ts.md"}
            },
            "required": ["path1", "path2"]
        })
    }
    fn is_read_only(&self) -> bool {
        true
    }

    async fn call(&self, input: Value, ctx: &ToolContext) -> Result<ToolOutput, ToolError> {
        let path1 = require_str(&input, "path1")?;
        let path2 = require_str(&input, "path2")?;
        let (old, new) = read_pair(&ctx.project_root, &path1, &path2)?;
        Ok(ToolOutput {
            content: build_summary(&path1, &path2, &old, &new),
            is_error: false,
        })
    }
}

fn read_pair(root: &Path, path1: &str, path2: &str) -> Result<(String, String), ToolError> {
    let old = std::fs::read_to_string(root.join(path1))
        .map_err(|_| ToolError::Execution(format!("path1 not found: {path1}")))?;
    let new = std::fs::read_to_string(root.join(path2))
        .map_err(|_| ToolError::Execution(format!("path2 not found: {path2}")))?;
    Ok((old, new))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn diff_counts_added_removed_unchanged() {
        let old = "第一行\n保持\n第二行\n末行\n";
        let new = "第一行\n保持\n改写后\n新加行\n末行\n";
        let (added, removed, unchanged, region) = diff_lines(old, new);
        assert_eq!(added, 2, "two new lines");
        assert_eq!(removed, 1, "one removed line");
        assert_eq!(unchanged, 3, "prefix+matched+suffix");
        let (start, end) = region.expect("region");
        assert_eq!((start, end), (3, 3), "old-side changed range");
    }

    #[test]
    fn diff_empty_versions() {
        assert_eq!(diff_lines("", ""), (0, 0, 0, None));
    }

    #[test]
    fn summary_marks_tail_change() {
        let old = "a\nb\n旧末行\n";
        let new = "a\nb\n新末行\n";
        let s = build_summary("chapters/chapter-001.md", "versions/v1.md", old, new);
        assert!(s.contains("末行已变化"), "got: {s}");
        assert!(s.contains("旧末行"));
        assert!(s.contains("新末行"));
    }

    #[test]
    fn summary_identical_versions() {
        let old = "a\nb\nc\n";
        let s = build_summary("p1.md", "p2.md", old, old);
        assert!(s.contains("内容未变化"), "got: {s}");
        assert!(s.contains("新增 0 行"));
    }
}
