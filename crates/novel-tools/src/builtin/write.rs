use super::super::{
    blocking, require_str, require_str_any, PermissionMode, Tool, ToolContext, ToolError,
    ToolOutput, ValidationError,
};
use async_trait::async_trait;
use serde_json::{json, Value};
use std::path::PathBuf;

pub struct WriteTool;

fn require_read_before_write(
    ctx: &ToolContext,
    full: &PathBuf,
    path: &str,
) -> Result<(), ToolError> {
    let mode = ctx.effective_permission_mode();
    if matches!(mode, PermissionMode::Auto | PermissionMode::Plan | PermissionMode::Unattended) {
        return Ok(());
    }
    if full.exists() && !ctx.was_read(full) {
        return Err(ToolError::Execution(format!(
            "Read {path} before overwriting (read-before-write policy)"
        )));
    }
    Ok(())
}

#[async_trait]
impl Tool for WriteTool {
    fn name(&self) -> &str {
        "Write"
    }
    fn description(&self) -> &str {
        "Write content to a file"
    }
    fn input_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "path": {"type": "string"},
                "content": {"type": "string"}
            },
            "required": ["path", "content"]
        })
    }
    fn is_concurrency_safe(&self) -> bool {
        false
    }

    fn validate_input(&self, input: &Value) -> Result<(), ValidationError> {
        require_str_any(input, &["path", "file_path"])?;
        require_str(input, "content")?;
        Ok(())
    }

    async fn call(&self, input: Value, ctx: &ToolContext) -> Result<ToolOutput, ToolError> {
        let path = require_str_any(&input, &["path", "file_path"])?;
        let content = require_str(&input, "content")?.to_string();
        let full = ctx.resolve_path(&path);
        ctx.validate_write_root(&full)?;
        ctx.validate_plan_mode_write_path(&path)?;
        require_read_before_write(ctx, &full, &path)?;
        if let Some(parent) = full.parent() {
            blocking::create_dir_all(parent.to_path_buf()).await?;
        }
        blocking::write(full.clone(), content).await?;
        Ok(ToolOutput {
            content: format!("Wrote {}", full.display()),
            is_error: false,
        })
    }
}
