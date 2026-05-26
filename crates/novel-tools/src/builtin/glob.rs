use super::super::{blocking, optional_search_root, optional_str_any, Tool, ToolContext, ToolError, ToolOutput};
use async_trait::async_trait;
use serde_json::{json, Value};
use std::path::PathBuf;
use walkdir::WalkDir;

pub struct GlobTool;

fn glob_sync(
    root: PathBuf,
    project_root: PathBuf,
    pattern: String,
) -> Result<ToolOutput, ToolError> {
    let needle = pattern.trim_matches('*');
    let mut files = Vec::new();
    for entry in WalkDir::new(&root).into_iter().filter_map(|e| e.ok()) {
        if !entry.file_type().is_file() {
            continue;
        }
        let name = entry.file_name().to_str().unwrap_or("");
        if pattern.contains('*') {
            if name.contains(needle) {
                files.push(
                    entry
                        .path()
                        .strip_prefix(&project_root)
                        .unwrap_or(entry.path())
                        .display()
                        .to_string(),
                );
            }
        } else if name == pattern {
            files.push(
                entry
                    .path()
                    .strip_prefix(&project_root)
                    .unwrap_or(entry.path())
                    .display()
                    .to_string(),
            );
        }
    }
    files.sort();
    Ok(ToolOutput {
        content: files.join("\n"),
        is_error: false,
    })
}

#[async_trait]
impl Tool for GlobTool {
    fn name(&self) -> &str {
        "Glob"
    }
    fn description(&self) -> &str {
        "Find files matching a glob pattern"
    }
    fn input_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "pattern": {"type": "string"},
                "search_root": {
                    "type": "string",
                    "description": "Directory to search under project root (default: project root)"
                }
            },
            "required": ["pattern"]
        })
    }
    fn is_read_only(&self) -> bool {
        true
    }

    async fn call(&self, input: Value, ctx: &ToolContext) -> Result<ToolOutput, ToolError> {
        let pattern = optional_str_any(&input, &["pattern", "glob_pattern", "glob"])
            .unwrap_or_else(|| "*".into());
        let root = optional_search_root(&input)
            .map(|p| ctx.resolve_path(&p))
            .unwrap_or_else(|| ctx.project_root.clone());
        let project_root = ctx.project_root.clone();
        blocking::run_blocking(move || glob_sync(root, project_root, pattern)).await
    }
}
