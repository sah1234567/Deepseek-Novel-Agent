use super::super::{blocking, optional_str_any, require_str_any, Tool, ToolContext, ToolError, ToolOutput};
use async_trait::async_trait;
use regex::Regex;
use serde_json::{json, Value};
use std::path::PathBuf;
use walkdir::WalkDir;

pub struct GrepTool;

const MAX_RESULT_CHARS: usize = 20_000;
const DEFAULT_HEAD_LIMIT: usize = 250;

fn grep_sync(
    pattern: String,
    search_root: PathBuf,
    project_root: PathBuf,
    glob_filter: Option<String>,
) -> Result<ToolOutput, ToolError> {
    let re =
        Regex::new(&pattern).map_err(|e| ToolError::Execution(format!("invalid regex: {e}")))?;
    let mut matches = Vec::new();
    let mut total_chars = 0usize;
    let mut truncated = false;

    for entry in WalkDir::new(&search_root)
        .into_iter()
        .filter_map(|e| e.ok())
        .filter(|e| e.file_type().is_file())
    {
        let path = entry.path();
        if let Some(ref g) = glob_filter {
            let name = path.file_name().and_then(|s| s.to_str()).unwrap_or("");
            if !name.contains(g.trim_matches('*')) {
                continue;
            }
        }
        let Ok(content) = std::fs::read_to_string(path) else {
            continue;
        };
        for (i, line) in content.lines().enumerate() {
            if re.is_match(line) {
                let formatted = format!(
                    "{}:{}:{}",
                    path.strip_prefix(&project_root).unwrap_or(path).display(),
                    i + 1,
                    line
                );
                if matches.len() >= DEFAULT_HEAD_LIMIT {
                    truncated = true;
                    break;
                }
                total_chars += formatted.len() + 1; // +1 for newline
                if total_chars > MAX_RESULT_CHARS {
                    truncated = true;
                    break;
                }
                matches.push(formatted);
            }
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
        "Search file contents with regex to locate lines before reading. \
         Results limited to 20 000 characters and 250 matches; use Read offset/limit on hit line numbers for context."
    }
    fn input_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "pattern": {"type": "string"},
                "path": {"type": "string"},
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
        let search_root = optional_str_any(&input, &["path", "file_path", "directory", "dir"])
            .map(|p| ctx.resolve_path(&p))
            .unwrap_or_else(|| ctx.project_root.clone());
        let glob_filter = input
            .get("glob")
            .and_then(|v| v.as_str())
            .map(|s| s.to_string());
        let project_root = ctx.project_root.clone();
        blocking::run_blocking(move || grep_sync(pattern, search_root, project_root, glob_filter))
            .await
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::PermissionMode;
    use std::io::Write;
    use tempfile::TempDir;

    fn test_ctx(tmp: &TempDir) -> ToolContext {
        ToolContext {
            permission_mode: PermissionMode::Normal,
            project_root: tmp.path().to_path_buf(),
            ..ToolContext::new(tmp.path().to_path_buf())
        }
    }

    fn write_file(dir: &std::path::Path, name: &str, content: &str) {
        let path = dir.join(name);
        let parent = path.parent().unwrap();
        std::fs::create_dir_all(parent).unwrap();
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
        for i in 0..300 {
            content.push_str(&format!("match line {i}\n"));
        }
        write_file(tmp.path(), "big.md", &content);
        let ctx = test_ctx(&tmp);
        let out = GrepTool
            .call(json!({"pattern": "match line"}), &ctx)
            .await
            .unwrap();
        assert!(out.content.contains("[truncated:"));
        // Should only have at most 250 matches
        let count = out.content.lines().filter(|l| l.contains(":match line")).count();
        assert!(count <= 250);
    }
}
