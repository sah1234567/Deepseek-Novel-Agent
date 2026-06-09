use super::super::{
    blocking, optional_search_root, require_str, Tool, ToolContext, ToolError, ToolOutput,
};
use async_trait::async_trait;
use grep_regex::RegexMatcher;
use grep_searcher::sinks::UTF8;
use grep_searcher::SearcherBuilder;
use ignore::WalkBuilder;
use serde_json::{json, Value};
use std::path::PathBuf;

pub struct GrepTool;

const MAX_RESULT_CHARS: usize = 20_000;
const DEFAULT_HEAD_LIMIT: usize = 80;

fn grep_sync_rg(
    pattern: String,
    search_root: PathBuf,
    project_root: PathBuf,
    glob_filter: Option<String>,
    head_limit: Option<usize>,
    offset: usize,
) -> Result<ToolOutput, ToolError> {
    let matcher = RegexMatcher::new(&pattern)
        .map_err(|e| ToolError::Execution(format!("invalid regex: {e}")))?;

    let mut searcher = SearcherBuilder::new().line_number(true).build();
    let mut matches: Vec<String> = Vec::new();
    let mut total_chars = 0usize;
    let mut truncated_by_count = false;
    let mut truncated_by_chars = false;
    let mut skipped = 0usize;

    // 0 = unlimited (use sparingly — large result sets waste context)
    let has_limit = head_limit != Some(0);
    let effective_limit = if head_limit == Some(0) {
        usize::MAX
    } else {
        head_limit.unwrap_or(DEFAULT_HEAD_LIMIT)
    };

    let mut walk = WalkBuilder::new(&search_root);
    walk.hidden(false);
    walk.ignore(true);

    for entry in walk.build().filter_map(|e| e.ok()) {
        let path = entry.path();
        if !path.is_file() {
            continue;
        }
        if let Some(ref g) = glob_filter {
            let name = path.file_name().and_then(|s| s.to_str()).unwrap_or("");
            if !name.contains(g.trim_matches('*')) {
                continue;
            }
        }

        let mut sink = UTF8(|line_num, line| {
            if truncated_by_count || truncated_by_chars {
                return Ok(false);
            }

            // Skip matches below offset — continue searching without counting toward limit
            if skipped < offset {
                skipped += 1;
                return Ok(true);
            }

            let rel = path
                .strip_prefix(&project_root)
                .unwrap_or(path)
                .display()
                .to_string()
                .replace('\\', "/");
            let formatted = format!("{rel}:{line_num}:{line}");

            if has_limit && matches.len() >= effective_limit {
                truncated_by_count = true;
                return Ok(false);
            }

            total_chars += formatted.len() + 1;
            if total_chars > MAX_RESULT_CHARS {
                truncated_by_chars = true;
                return Ok(false);
            }

            matches.push(formatted);
            Ok(true)
        });

        if searcher.search_path(&matcher, path, &mut sink).is_err() {
            continue;
        }
        if truncated_by_count || truncated_by_chars {
            break;
        }
    }

    let mut content = matches.join("\n");

    // Build pagination info — only show limit when head_limit was the truncation trigger
    let mut parts: Vec<String> = Vec::new();
    if truncated_by_count {
        parts.push(format!("limit: {}", matches.len().min(effective_limit)));
    }
    if offset > 0 {
        parts.push(format!("offset: {offset}"));
    }

    if !parts.is_empty() {
        content.push_str(&format!(
            "\n\n[Showing results with pagination = {}]",
            parts.join(", ")
        ));
    } else if truncated_by_chars {
        content.push_str(
            "\n\n[Showing partial results — character limit reached. \
             Narrow pattern or use glob to reduce matches.]",
        );
    }

    Ok(ToolOutput {
        content,
        is_error: false,
    })
}

#[async_trait]
impl Tool for GrepTool {
    fn name(&self) -> &str {
        "Grep"
    }
    fn description(&self) -> &str {
        "Search file contents with regex (ripgrep) to locate lines before reading. \
         Supports full regex syntax. Filter files with glob parameter. \
         Results default to 80 matches; use head_limit/offset for pagination. \
         Pass head_limit=0 for unlimited (use sparingly)."
    }
    fn usage_hint(&self) -> &str {
        "First step for read economy. Paginate with offset/head_limit. Then Read offset/limit on hit line numbers."
    }
    fn input_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "pattern": {"type": "string"},
                "search_root": {
                    "type": "string",
                    "description": "Directory to search under project root (default: project root)"
                },
                "glob": {
                    "type": "string",
                    "description": "Glob pattern to filter files (e.g. \"*.js\", \"*.{ts,tsx}\") - maps to rg --glob"
                },
                "head_limit": {
                    "type": "integer",
                    "description": "Limit output to first N entries. Defaults to 80 when unspecified. Pass 0 for unlimited (use sparingly — large result sets waste context)."
                },
                "offset": {
                    "type": "integer",
                    "description": "Skip first N entries before applying head_limit. Defaults to 0. Use with head_limit to paginate through results."
                }
            },
            "required": ["pattern"]
        })
    }
    fn is_read_only(&self) -> bool {
        true
    }

    fn max_output_lines(&self, _input: &Value) -> Option<usize> {
        Some(crate::read_economy::GREP_MAX_LINES)
    }

    fn output_limit_exceeded_hint(&self) -> &'static str {
        "Narrow the pattern, add a glob filter, or use head_limit/offset for pagination."
    }

    async fn call(&self, input: Value, ctx: &ToolContext) -> Result<ToolOutput, ToolError> {
        let pattern = require_str(&input, "pattern")?.to_string();
        let search_root = optional_search_root(&input)
            .map(|p| ctx.resolve_path(&p))
            .unwrap_or_else(|| ctx.project_root.clone());
        let glob_filter = input
            .get("glob")
            .and_then(|v| v.as_str())
            .map(|s| s.to_string());
        let head_limit = input
            .get("head_limit")
            .and_then(|v| v.as_u64())
            .map(|n| n as usize);
        let offset = input
            .get("offset")
            .and_then(|v| v.as_u64())
            .map(|n| n as usize)
            .unwrap_or(0);
        let project_root = ctx.project_root.clone();
        blocking::run_blocking(move || {
            grep_sync_rg(
                pattern,
                search_root,
                project_root,
                glob_filter,
                head_limit,
                offset,
            )
        })
        .await
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::PermissionMode;
    use std::io::Write;
    use std::path::Path;
    use tempfile::TempDir;

    fn test_ctx(tmp: &TempDir) -> ToolContext {
        ToolContext {
            permission_mode: PermissionMode::Normal,
            project_root: tmp.path().to_path_buf(),
            ..ToolContext::new(tmp.path().to_path_buf())
        }
    }

    fn write_file(dir: &Path, name: &str, content: &str) {
        let path = dir.join(name);
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).unwrap();
        }
        let mut f = std::fs::File::create(&path).unwrap();
        f.write_all(content.as_bytes()).unwrap();
    }

    #[tokio::test(flavor = "current_thread")]
    async fn basic_grep_search() {
        let tmp = TempDir::new().unwrap();
        write_file(tmp.path(), "a.md", "hello world\nfoo bar\nhello again");
        let ctx = test_ctx(&tmp);
        let out = GrepTool
            .call(json!({"pattern": "hello"}), &ctx)
            .await
            .unwrap();
        assert!(out.content.contains("hello world"));
        assert!(out.content.contains("hello again"));
    }

    #[tokio::test(flavor = "current_thread")]
    async fn truncates_with_pagination_message() {
        let tmp = TempDir::new().unwrap();
        let mut content = String::new();
        for i in 0..100 {
            content.push_str(&format!("match line {i}\n"));
        }
        write_file(tmp.path(), "big.md", &content);
        let ctx = test_ctx(&tmp);
        let out = GrepTool
            .call(json!({"pattern": "match line"}), &ctx)
            .await
            .unwrap();
        assert!(out.content.contains("[Showing results with pagination"));
        assert!(out.content.contains("limit: 80"));
    }

    #[tokio::test(flavor = "current_thread")]
    async fn no_pagination_message_when_under_limit() {
        let tmp = TempDir::new().unwrap();
        let mut content = String::new();
        for i in 0..10 {
            content.push_str(&format!("match line {i}\n"));
        }
        write_file(tmp.path(), "small.md", &content);
        let ctx = test_ctx(&tmp);
        let out = GrepTool
            .call(json!({"pattern": "match line"}), &ctx)
            .await
            .unwrap();
        assert!(!out.content.contains("pagination"));
    }

    #[tokio::test(flavor = "current_thread")]
    async fn offset_skips_results_and_shows_pagination() {
        let tmp = TempDir::new().unwrap();
        let mut content = String::new();
        for i in 0..50 {
            content.push_str(&format!("match line {i}\n"));
        }
        write_file(tmp.path(), "test.md", &content);
        let ctx = test_ctx(&tmp);

        let page1 = GrepTool
            .call(json!({"pattern": "match line", "head_limit": 10}), &ctx)
            .await
            .unwrap();
        assert!(page1.content.contains("line 0"));
        assert!(!page1.content.contains("line 10"));
        assert!(page1
            .content
            .contains("[Showing results with pagination = limit: 10]"));

        let page2 = GrepTool
            .call(
                json!({"pattern": "match line", "head_limit": 10, "offset": 10}),
                &ctx,
            )
            .await
            .unwrap();
        assert!(!page2.content.contains("line 0"));
        assert!(page2.content.contains("line 10"));
        assert!(!page2.content.contains("line 20"));
        assert!(page2.content.contains("offset: 10"));
        assert!(page2.content.contains("limit: 10"));
    }

    #[tokio::test(flavor = "current_thread")]
    async fn offset_past_end_shows_offset_without_limit() {
        let tmp = TempDir::new().unwrap();
        let mut content = String::new();
        for i in 0..10 {
            content.push_str(&format!("match line {i}\n"));
        }
        write_file(tmp.path(), "small.md", &content);
        let ctx = test_ctx(&tmp);

        let out = GrepTool
            .call(
                json!({"pattern": "match line", "head_limit": 10, "offset": 5}),
                &ctx,
            )
            .await
            .unwrap();
        // 5–9 = 5 matches, under the limit → no "limit:" in pagination
        assert!(!out.content.contains("limit:"));
        assert!(out.content.contains("offset: 5"));
    }

    #[tokio::test(flavor = "current_thread")]
    async fn head_limit_zero_is_unlimited() {
        let tmp = TempDir::new().unwrap();
        let mut content = String::new();
        for i in 0..300 {
            content.push_str(&format!("match line {i}\n"));
        }
        write_file(tmp.path(), "big.md", &content);
        let ctx = test_ctx(&tmp);

        let out = GrepTool
            .call(json!({"pattern": "match line", "head_limit": 0}), &ctx)
            .await
            .unwrap();
        // All 300 should be in the output (no limit), no pagination message
        assert!(!out.content.contains("pagination"));
        assert!(out.content.contains("line 299"));
    }

    #[tokio::test(flavor = "current_thread")]
    async fn explicit_head_limit_overrides_default() {
        let tmp = TempDir::new().unwrap();
        let mut content = String::new();
        for i in 0..100 {
            content.push_str(&format!("match line {i}\n"));
        }
        write_file(tmp.path(), "test.md", &content);
        let ctx = test_ctx(&tmp);

        let out = GrepTool
            .call(json!({"pattern": "match line", "head_limit": 30}), &ctx)
            .await
            .unwrap();
        assert!(out.content.contains("limit: 30"));
        assert!(!out.content.contains("line 30"));
    }
}
