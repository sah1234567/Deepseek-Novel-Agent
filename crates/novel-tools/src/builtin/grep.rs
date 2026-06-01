use super::super::{
    blocking, optional_search_root, require_str_any, Tool, ToolContext, ToolError, ToolOutput,
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
) -> Result<ToolOutput, ToolError> {
    let matcher = RegexMatcher::new(&pattern)
        .map_err(|e| ToolError::Execution(format!("invalid regex: {e}")))?;

    let mut searcher = SearcherBuilder::new().line_number(true).build();
    let mut matches = Vec::new();
    let mut total_chars = 0usize;
    let mut truncated = false;

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
            if truncated {
                return Ok(false);
            }
            let rel = path
                .strip_prefix(&project_root)
                .unwrap_or(path)
                .display()
                .to_string()
                .replace('\\', "/");
            let formatted = format!("{rel}:{line_num}:{line}");
            if matches.len() >= DEFAULT_HEAD_LIMIT {
                truncated = true;
                return Ok(false);
            }
            total_chars += formatted.len() + 1;
            if total_chars > MAX_RESULT_CHARS {
                truncated = true;
                return Ok(false);
            }
            matches.push(formatted);
            Ok(true)
        });

        if searcher.search_path(&matcher, path, &mut sink).is_err() {
            continue;
        }
        if truncated {
            break;
        }
    }

    let mut content = matches.join("\n");
    if truncated {
        let shown = matches.len();
        content.push_str(&format!(
            "\n[truncated: showing first {shown} matches, exceeded limit]"
        ));
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
         Results limited to 20 000 characters and 80 matches; use Read offset/limit on hit line numbers."
    }
    fn usage_hint(&self) -> &str {
        "First step for read economy. >80 match lines rejected. Then Read offset/limit on hit line numbers."
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
                "glob": {"type": "string"}
            },
            "required": ["pattern"]
        })
    }
    fn is_read_only(&self) -> bool {
        true
    }

    async fn call(&self, input: Value, ctx: &ToolContext) -> Result<ToolOutput, ToolError> {
        let pattern = require_str_any(&input, &["pattern", "query", "regex"])?.to_string();
        let search_root = optional_search_root(&input)
            .map(|p| ctx.resolve_path(&p))
            .unwrap_or_else(|| ctx.project_root.clone());
        let glob_filter = input
            .get("glob")
            .and_then(|v| v.as_str())
            .map(|s| s.to_string());
        let project_root = ctx.project_root.clone();
        blocking::run_blocking(move || {
            grep_sync_rg(pattern, search_root, project_root, glob_filter)
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
    async fn truncates_on_many_results() {
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
        assert!(out.content.contains("[truncated:"));
    }
}
