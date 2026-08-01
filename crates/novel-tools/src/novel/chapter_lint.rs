//! ChapterLint — deterministic lint layer for chapter files.
//!
//! Pure-function mechanical checks so the LLM audit focuses on semantics instead of
//! counting. Anti-AI-flavor parameters are
//! transferred verbatim from skills/audit-craft/SKILL.md (反 AI 味七项):
//! `然后` >3/章, `不是…(而)是…` any, 破折号 >1/章, 排比 ≥2 组, 结构化序号 any,
//! Markdown 标记 any, 环境罗列 ≥3 句 (heuristic — candidate only).
//! Counting is per matching line (same rule as the Grep-based audit).

use super::common::list_chapter_files;
use crate::{require_str, Tool, ToolContext, ToolError, ToolOutput};
use async_trait::async_trait;
use regex::Regex;
use serde_json::{json, Value};

pub struct ChapterLintTool;

const THEN_MAX: usize = 3;
const DASH_MAX: usize = 1;
const PARALLEL_MAX: usize = 1; // ≥2 组超标 → >1 达标
const ENV_ROWS_MIN: usize = 3;

fn re(pattern: &str) -> Regex {
    Regex::new(pattern).expect("chapter lint pattern must compile")
}

/// Per-line match counts (Grep counts matching lines, not occurrences).
fn count_matching_lines(content: &str, pattern: &Regex) -> usize {
    content.lines().filter(|l| pattern.is_match(l)).count()
}

fn lint_ai_flavor(content: &str) -> Vec<String> {
    let mut out = Vec::new();
    let then_count = count_matching_lines(content, &re(r"然后"));
    if then_count > THEN_MAX {
        out.push(format!("「然后」{then_count} 次(> {THEN_MAX} 超标)"));
    }
    let not_is_count = count_matching_lines(content, &re(r"不是.{0,15}是"));
    if not_is_count > 0 {
        out.push(format!("「不是…(而)是…」{not_is_count} 次(出现即标)"));
    }
    let dash_count = count_matching_lines(content, &re(r"——|--|—|–| - "));
    if dash_count > DASH_MAX {
        out.push(format!("破折号 {dash_count} 次(> {DASH_MAX} 超标)"));
    }
    let parallel_count = count_matching_lines(
        content,
        &re(r"(首先|其次|再次|最后|一方面|另一方面|与此同时)"),
    );
    if parallel_count > PARALLEL_MAX {
        out.push(format!("排比标记 {parallel_count} 组(≥2 超标)"));
    }
    let seq_count =
        count_matching_lines(content, &re(r"^(一|二|三|四|五|六|七|八|九|十)[、\.,，]"));
    if seq_count > 0 {
        out.push(format!("结构化序号 {seq_count} 次(出现即标)"));
    }
    let markdown_count = count_matching_lines(content, &re(r"^#{1,3}\s|\*\*|`|\*[^*\s]|~~|__"));
    if markdown_count > 0 {
        out.push(format!("Markdown 标记 {markdown_count} 次(出现即标)"));
    }
    // 环境罗列 — heuristic: ≥3 consecutive non-dialogue, non-action lines at chapter start
    // or transitions. Conservative: report only the first candidate run.
    let env_heuristic = lint_env_roster(content);
    if let Some(hint) = env_heuristic {
        out.push(hint);
    }
    out
}

/// Candidate run of ≥3 consecutive description-only lines (展厅式描写候选，需人工确认).
fn lint_env_roster(content: &str) -> Option<String> {
    let mut run = 0usize;
    for line in content.lines() {
        let l = line.trim();
        if l.is_empty() {
            continue;
        }
        let is_dialogue = l.starts_with('「') || l.starts_with('"') || l.starts_with('『');
        let is_heading = l.starts_with('#') || l.starts_with('|');
        if !is_dialogue && !is_heading {
            run += 1;
        } else {
            run = 0;
        }
        if run >= ENV_ROWS_MIN {
            return Some(format!(
                "连续 ≥{ENV_ROWS_MIN} 行叙述段(展厅式描写候选，需人工确认)"
            ));
        }
    }
    None
}

/// Chapter number continuity: files 1..=max must all exist.
fn lint_continuity(project_root: &std::path::Path) -> Option<String> {
    let mut chs: Vec<u32> = list_chapter_files(project_root)
        .into_iter()
        .map(|(c, _)| c)
        .collect();
    if chs.is_empty() {
        return None;
    }
    chs.sort_unstable();
    let mut missing = Vec::new();
    let max = *chs.last().unwrap_or(&0);
    for n in 1..=max {
        if !chs.binary_search(&n).is_ok() {
            missing.push(n);
        }
    }
    if missing.is_empty() {
        None
    } else {
        Some(format!(
            "章节跳号: {}（最大 Ch{max}）",
            missing
                .iter()
                .map(|n| format!("Ch{n}"))
                .collect::<Vec<_>>()
                .join(", ")
        ))
    }
}

/// Semi-deterministic: outline 出场人物清单 vs body appearance (character names Grep).
fn lint_outline_characters(project_root: &std::path::Path, chapter: u32) -> Option<String> {
    let outline_rel = format!("knowledge/plot/细纲/chapter-{chapter:03}-细纲.md");
    let outline_path = project_root.join(&outline_rel);
    let Ok(outline) = std::fs::read_to_string(&outline_path) else {
        return None; // no outline → skip (not an error)
    };
    let body_path = project_root.join(format!("chapters/chapter-{chapter:03}.md"));
    let Ok(body) = std::fs::read_to_string(&body_path) else {
        return None;
    };
    // Extract names from the 出场人物清单 table (2nd column).
    let mut names: Vec<String> = Vec::new();
    let mut in_cast = false;
    for line in outline.lines() {
        let l = line.trim();
        if l.starts_with("## 出场人物清单") {
            in_cast = true;
            continue;
        }
        if in_cast {
            if l.starts_with("## ") {
                break;
            }
            if !l.starts_with('|') || l.contains("人物") || l.contains("---") {
                continue;
            }
            let cells: Vec<&str> = l.split('|').map(|s| s.trim()).collect();
            if let Some(name) = cells.get(1) {
                let name = name.trim();
                if !name.is_empty() && name != "本章作用" {
                    names.push(name.to_string());
                }
            }
        }
    }
    if names.is_empty() {
        return None;
    }
    let missing: Vec<&String> = names
        .iter()
        .filter(|n| !body.contains(n.as_str()))
        .collect();
    if missing.is_empty() {
        None
    } else {
        let names = missing
            .iter()
            .map(|s| s.as_str())
            .collect::<Vec<_>>()
            .join(", ");
        Some(format!("细纲出场人物未在正文出现(半确定): {names}"))
    }
}

fn lint_chapter(project_root: &std::path::Path, chapter: u32) -> Result<String, ToolError> {
    let path = project_root.join(format!("chapters/chapter-{chapter:03}.md"));
    let content = std::fs::read_to_string(&path).map_err(|_| {
        ToolError::Execution(format!(
            "chapter file not found: chapters/chapter-{chapter:03}.md"
        ))
    })?;
    let mut lines = vec![format!("## ChapterLint (Ch{chapter})")];

    let flavor = lint_ai_flavor(&content);
    if flavor.is_empty() {
        lines.push("- AI 味: 全部达标".to_string());
    } else {
        lines.push(format!("- AI 味: {}", flavor.join(" | ")));
    }

    match lint_continuity(project_root) {
        Some(msg) => lines.push(format!("- 连续性: {msg}")),
        None => lines.push("- 章节连续性: OK".to_string()),
    }

    if let Some(msg) = lint_outline_characters(project_root, chapter) {
        lines.push(format!("- 一致性: {msg}"));
    }

    Ok(lines.join("\n"))
}

#[async_trait]
impl Tool for ChapterLintTool {
    fn name(&self) -> &str {
        "ChapterLint"
    }
    fn description(&self) -> &str {
        "Deterministic lint of a chapter file — anti-AI-flavor seven-item counts (same parameters as audit-craft), chapter-number continuity, and semi-deterministic outline-cast vs body appearance check"
    }
    fn input_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "chapter": {"type": "string", "description": "Chapter number to lint, e.g. \"12\""}
            },
            "required": ["chapter"]
        })
    }
    fn is_read_only(&self) -> bool {
        true
    }

    async fn call(&self, input: Value, ctx: &ToolContext) -> Result<ToolOutput, ToolError> {
        let chapter_str = require_str(&input, "chapter")?;
        let chapter: u32 = chapter_str.trim().parse().map_err(|_| {
            ToolError::Execution(format!("chapter must be an integer, got: {chapter_str}"))
        })?;
        let content = lint_chapter(&ctx.project_root, chapter)?;
        Ok(ToolOutput {
            content,
            is_error: false,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{PermissionMode, ToolContext};
    use tempfile::TempDir;

    fn write(root: &std::path::Path, rel: &str, content: &str) {
        let path = root.join(rel);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, content).unwrap();
    }

    fn ctx_for(tmp: &TempDir) -> ToolContext {
        ToolContext {
            permission_mode: PermissionMode::Auto,
            project_root: tmp.path().to_path_buf(),
            ..ToolContext::new(tmp.path().to_path_buf())
        }
    }

    #[tokio::test(flavor = "current_thread")]
    async fn flags_ai_flavor_and_continuity() {
        let tmp = TempDir::new().unwrap();
        // chapter-001: 4 行「然后」(>3) + 2 行破折号 (>1) + 1 行「不是…是」(出现即标)
        write(
            tmp.path(),
            "chapters/chapter-001.md",
            "他然后走了。\n她然后笑了——门开了。\n然后天黑了——风停了。\n陈默推门而入，然后门开了，不是风，是有人推的。\n",
        );
        // chapter-003 exists but chapter-002 missing → jump
        write(tmp.path(), "chapters/chapter-003.md", "第三段正文。\n");
        write(
            tmp.path(),
            "knowledge/plot/细纲/chapter-001-细纲.md",
            "## 出场人物清单\n| 人物 | 本章作用 | POV 场景 |\n|------|---------|---------|\n| 陈默 | 主角 | 场景1 |\n| 苏雨桐 | 配角 | 场景2 |\n",
        );

        let tool = ChapterLintTool;
        let out = tool
            .call(json!({"chapter": "1"}), &ctx_for(&tmp))
            .await
            .unwrap();
        assert!(
            out.content.contains("「然后」4 次"),
            "then: {}",
            out.content
        );
        assert!(out.content.contains("破折号"), "dash: {}", out.content);
        assert!(out.content.contains("不是"), "not-is: {}", out.content);
        assert!(out.content.contains("Ch2"), "continuity: {}", out.content);
        assert!(
            out.content
                .contains("一致性: 细纲出场人物未在正文出现(半确定): 苏雨桐"),
            "only 苏雨桐 missing, got: {}",
            out.content
        );
        assert!(
            !out.content.contains("陈默, 苏雨桐"),
            "陈默 appears in body"
        );
    }

    #[tokio::test(flavor = "current_thread")]
    async fn clean_chapter_passes() {
        let tmp = TempDir::new().unwrap();
        write(
            tmp.path(),
            "chapters/chapter-001.md",
            "陈默推开门。\n「你来了。」他说。\n她点头，接过信。\n",
        );
        let tool = ChapterLintTool;
        let out = tool
            .call(json!({"chapter": "1"}), &ctx_for(&tmp))
            .await
            .unwrap();
        assert!(out.content.contains("全部达标"), "got: {}", out.content);
        assert!(out.content.contains("连续性: OK"));
    }

    #[test]
    fn env_roster_detects_three_description_rows() {
        let body = "月光洒在庭院里。\n石阶上积着薄霜。\n远处的灯火忽明忽暗。\n「有人吗？」\n";
        assert!(lint_env_roster(body).is_some());
        assert!(lint_env_roster("「对话」\n「对话」\n").is_none());
    }
}
