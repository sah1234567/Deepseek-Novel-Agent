use super::super::{
    blocking, extract_file_path, file_mtime_secs, require_str, Tool, ToolContext, ToolError,
    ToolOutput, ValidationError,
};
use async_trait::async_trait;
use serde_json::{json, Value};

pub struct WriteTool;

#[async_trait]
impl Tool for WriteTool {
    fn name(&self) -> &str {
        "Write"
    }

    fn description(&self) -> &str {
        "Write content to a file (create or full overwrite). For partial changes to an existing file, use Edit instead."
    }

    fn usage_hint(&self) -> &str {
        "New files or full overwrite only; existing-file patches → Edit. Read before overwrite in Normal mode."
    }

    fn input_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "file_path": {
                    "type": "string",
                    "description": "Relative path under project root"
                },
                "content": {
                    "type": "string",
                    "description": "Full file content to write"
                }
            },
            "required": ["file_path", "content"]
        })
    }

    fn is_concurrency_safe(&self) -> bool {
        false
    }

    fn validate_input(&self, input: &Value) -> Result<(), ValidationError> {
        extract_file_path(input)?;
        require_str(input, "content")?;
        Ok(())
    }

    async fn call(&self, input: Value, ctx: &ToolContext) -> Result<ToolOutput, ToolError> {
        let path = extract_file_path(&input)?;
        let content = require_str(&input, "content")?.to_string();
        let full = ctx.resolve_path(&path);
        ctx.validate_write_root(&full)?;
        ctx.validate_plan_mode_write_path(&path)?;
        ctx.require_read_before_write(&full, &path, "overwriting", true)?;

        if full.exists() {
            let existing = blocking::read_to_string(full.clone()).await?;
            if let Some(entry) = ctx.read_cache_entry(&full) {
                let meta = tokio::fs::metadata(&full).await.map_err(ToolError::Io)?;
                entry.check_fresh_for_disk(file_mtime_secs(&meta), &existing, "overwriting")?;
            }
        }

        if let Some(parent) = full.parent() {
            blocking::create_dir_all(parent.to_path_buf()).await?;
        }
        blocking::write(full.clone(), content.clone()).await?;

        let mtime = tokio::fs::metadata(&full)
            .await
            .map(|m| file_mtime_secs(&m))
            .unwrap_or(0);
        ctx.refresh_cache_after_write(&full, &content, mtime);

        Ok(ToolOutput {
            content: format!("Wrote {}", full.display()),
            is_error: false,
        })
    }
}
