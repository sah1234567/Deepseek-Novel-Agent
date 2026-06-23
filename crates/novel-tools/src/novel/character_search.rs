use crate::{require_str, Tool, ToolContext, ToolError, ToolOutput};
use async_trait::async_trait;
use novel_knowledge::{parse_frontmatter, CharacterFrontmatter};
use regex::Regex;
use serde::Serialize;
use serde_json::{json, Value};
use walkdir::WalkDir;

#[derive(Debug, Clone, Serialize)]
struct MatchHit {
    file: String,
    line: u32,
    snippet: String,
}

#[derive(Debug, Serialize)]
struct CharacterProfile {
    name: String,
    frontmatter: CharacterFrontmatter,
    matches: Vec<MatchHit>,
}

#[derive(Debug, Serialize)]
pub(crate) struct CharacterSearchResult {
    query: String,
    field: String,
    matches: Vec<MatchHit>,
    profiles: Vec<CharacterProfile>,
}

fn field_headings(field: &str) -> Option<Vec<&'static str>> {
    match field {
        "identity" => Some(vec!["## 身份", "身份演变日志"]),
        "personality" => Some(vec!["## 性格", "性格演变日志"]),
        "relationships" => Some(vec!["## 关系", "关系演变日志"]),
        "appearances" => Some(vec!["出场记录日志"]),
        "secrets" => Some(vec!["## 秘密", "秘密演变日志"]),
        _ => None,
    }
}

fn extract_sections(content: &str, headings: &[&str]) -> String {
    let mut chunks = Vec::new();
    for heading in headings {
        let marker = if heading.starts_with("## ") {
            heading.to_string()
        } else {
            format!("## {heading}")
        };
        if let Some(start) = content.find(&marker) {
            let rest = &content[start..];
            let end = rest[marker.len()..]
                .find("\n## ")
                .map(|i| i + marker.len())
                .unwrap_or(rest.len());
            chunks.push(rest[..end].trim());
        } else if content.contains(heading) {
            if let Some(start) = content.find(heading) {
                let rest = &content[start..];
                let end = rest[heading.len()..]
                    .find("\n## ")
                    .map(|i| i + heading.len())
                    .unwrap_or(rest.len());
                chunks.push(rest[..end].trim());
            }
        }
    }
    chunks.join("\n\n")
}

fn searchable_text(content: &str, field: Option<&str>) -> String {
    if let Some(f) = field {
        if let Some(headings) = field_headings(f) {
            let section = extract_sections(content, &headings);
            if !section.is_empty() {
                return section;
            }
        }
    }
    if content.starts_with("---") {
        if let Some(end) = content.find("\n---\n") {
            return content[end + 5..].to_string();
        }
    }
    content.to_string()
}

fn find_line_matches(
    re: &Regex,
    file_name: &str,
    content: &str,
    search_in: &str,
    context_lines: usize,
) -> Vec<MatchHit> {
    let content_offset = content.find(search_in).unwrap_or(0);
    let lines_before: u32 = content[..content_offset]
        .lines()
        .count()
        .try_into()
        .unwrap_or(0);
    let lines: Vec<&str> = search_in.lines().collect();
    let mut file_matches = Vec::new();
    for (i, line) in lines.iter().enumerate() {
        if re.is_match(line) {
            let start = i.saturating_sub(context_lines);
            let end = (i + context_lines + 1).min(lines.len());
            file_matches.push(MatchHit {
                file: file_name.to_string(),
                line: lines_before + (i as u32) + 1,
                snippet: lines[start..end].join("\n"),
            });
        }
    }
    file_matches
}

fn profile_from_matches(content: &str, file_matches: Vec<MatchHit>) -> Option<CharacterProfile> {
    if file_matches.is_empty() {
        return None;
    }
    let (fm, _) = parse_frontmatter::<CharacterFrontmatter>(content).ok()?;
    Some(CharacterProfile {
        name: fm.name.clone(),
        frontmatter: fm,
        matches: file_matches,
    })
}

pub(crate) fn search_characters_in_dir(
    chars_dir: &std::path::Path,
    field: &str,
    query: &str,
    context_lines: usize,
) -> Result<CharacterSearchResult, ToolError> {
    let re = Regex::new(&regex::escape(query))
        .map_err(|e| ToolError::Execution(format!("invalid query regex: {e}")))?;
    let mut all_matches = Vec::new();
    let mut profiles = Vec::new();

    if !chars_dir.exists() {
        return Ok(CharacterSearchResult {
            query: query.to_string(),
            field: field.into(),
            matches: vec![],
            profiles: vec![],
        });
    }

    for entry in WalkDir::new(chars_dir)
        .max_depth(1)
        .into_iter()
        .filter_map(|e| e.ok())
    {
        let path = entry.path();
        if path.extension().and_then(|e| e.to_str()) != Some("md") {
            continue;
        }
        if path.file_name().and_then(|s| s.to_str()) == Some("_template.md") {
            continue;
        }
        let Ok(content) = std::fs::read_to_string(path) else {
            continue;
        };
        let file_name = path
            .file_name()
            .map(|s| s.to_string_lossy().to_string())
            .unwrap_or_default();
        let search_in = searchable_text(&content, if field == "all" { None } else { Some(field) });
        let file_matches = find_line_matches(&re, &file_name, &content, &search_in, context_lines);
        all_matches.extend(file_matches.iter().cloned());
        if let Some(profile) = profile_from_matches(&content, file_matches) {
            profiles.push(profile);
        }
    }

    Ok(CharacterSearchResult {
        query: query.to_string(),
        field: field.into(),
        matches: all_matches,
        profiles,
    })
}

pub struct CharacterSearchTool;

#[async_trait]
impl Tool for CharacterSearchTool {
    fn name(&self) -> &str {
        "CharacterSearch"
    }
    fn description(&self) -> &str {
        "Search character profiles in knowledge/characters"
    }
    fn input_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "query": {"type": "string"},
                "field": {"type": "string", "enum": ["all", "identity", "personality", "relationships", "appearances", "secrets"]},
                "context": {"type": "integer"}
            },
            "required": ["query"]
        })
    }
    fn is_read_only(&self) -> bool {
        true
    }

    fn usage_hint(&self) -> &str {
        "Character search across all character cards. For current-state lookup, pair with Grep `## 当前状态快照` or `^\\| Ch` on the matched files."
    }

    fn max_output_lines(&self, _input: &Value) -> Option<usize> {
        Some(crate::read_economy::KNOWLEDGE_MAX_LINES)
    }

    fn output_limit_exceeded_hint(&self) -> &'static str {
        "Narrow the search scope or use a more specific query."
    }

    async fn call(&self, input: Value, ctx: &ToolContext) -> Result<ToolOutput, ToolError> {
        let query = require_str(&input, "query")?;
        let field = input.get("field").and_then(|v| v.as_str()).unwrap_or("all");
        let context_lines = input.get("context").and_then(|v| v.as_u64()).unwrap_or(5) as usize;
        let chars_dir = ctx.project_root.join("knowledge/characters");
        let result = search_characters_in_dir(&chars_dir, field, &query, context_lines)?;
        Ok(ToolOutput {
            content: serde_json::to_string_pretty(&result)
                .map_err(|e| ToolError::Internal(e.to_string()))?,
            is_error: false,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{PermissionMode, ToolContext};
    use tempfile::TempDir;

    #[test]
    fn extract_sections_finds_heading_block() {
        let text = "## 性格演变日志\n| Ch1 | 天真 |\n\n## 其他\nx";
        let out = extract_sections(text, &["性格演变日志"]);
        assert!(out.contains("天真"));
    }

    #[test]
    fn search_characters_in_dir_empty_when_missing() {
        let tmp = TempDir::new().unwrap();
        let out = search_characters_in_dir(&tmp.path().join("knowledge/characters"), "all", "x", 2)
            .unwrap();
        assert!(out.matches.is_empty());
    }

    #[tokio::test(flavor = "current_thread")]
    async fn json_output_with_profiles() {
        let tmp = TempDir::new().unwrap();
        std::fs::create_dir_all(tmp.path().join("knowledge/characters")).unwrap();
        std::fs::write(
            tmp.path().join("knowledge/characters/林若烟.md"),
            "---\nname: 林若烟\ncategory: human\nfirstAppearance: Ch1\nlastUpdate: Ch1\nstatus: alive\npovCharacter: true\n---\n\n## 性格演变日志\n| Ch1 | 天真 |\n",
        )
        .unwrap();
        let tool = CharacterSearchTool;
        let ctx = ToolContext {
            permission_mode: PermissionMode::Auto,
            project_root: tmp.path().to_path_buf(),
            ..ToolContext::new(tmp.path().to_path_buf())
        };
        let out = tool
            .call(json!({"query": "天真", "field": "personality"}), &ctx)
            .await
            .unwrap();
        assert!(out.content.contains("\"profiles\""));
        assert!(out.content.contains("林若烟"));
    }

    #[tokio::test(flavor = "current_thread")]
    async fn line_number_is_absolute_after_frontmatter() {
        let tmp = TempDir::new().unwrap();
        std::fs::create_dir_all(tmp.path().join("knowledge/characters")).unwrap();
        // 10 lines of frontmatter + blank, then content starting at line 12
        let body = "---\nname: 林若烟\ncategory: human\nfirstAppearance: Ch1\nlastUpdate: Ch1\nstatus: alive\npovCharacter: true\n---\n\n## 性格演变日志\n| 章节 | 性格 | 触发事件 |\n|------|------|---------|\n| Ch1 | 天真 | 入门 |";
        let _body_lines: Vec<&str> = body.lines().collect();
        std::fs::write(tmp.path().join("knowledge/characters/林若烟.md"), body).unwrap();
        let tool = CharacterSearchTool;
        let ctx = ToolContext {
            permission_mode: PermissionMode::Auto,
            project_root: tmp.path().to_path_buf(),
            ..ToolContext::new(tmp.path().to_path_buf())
        };
        let out = tool.call(json!({"query": "天真"}), &ctx).await.unwrap();
        assert!(out.content.contains("林若烟"));
        // "天真" is on the last line; line number should be absolute, not 1-based
        // from the section. With 10 frontmatter lines + 1 blank + 1 heading + 1 table header
        // + 1 separator = 14 lines before data, so line should be ~14-15
        let parsed: serde_json::Value = serde_json::from_str(&out.content).unwrap();
        let line = parsed["profiles"][0]["matches"][0]["line"]
            .as_u64()
            .unwrap();
        assert!(line >= 10, "line should be absolute (>=10), got {line}");
    }
}
