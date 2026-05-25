use super::super::{
    blocking, require_str, require_str_any, PermissionMode, Tool, ToolContext, ToolError,
    ToolOutput, ValidationError,
};
use async_trait::async_trait;
use serde_json::{json, Value};
use std::path::PathBuf;

pub struct EditTool;

fn require_read_before_write(
    ctx: &ToolContext,
    full: &PathBuf,
    path: &str,
) -> Result<(), ToolError> {
    let mode = ctx.effective_permission_mode();
    if matches!(mode, PermissionMode::Auto | PermissionMode::Plan | PermissionMode::Unattended) {
        return Ok(());
    }
    if !ctx.was_read(full) {
        return Err(ToolError::Execution(format!(
            "Read {path} before editing (read-before-write policy)"
        )));
    }
    Ok(())
}

#[async_trait]
impl Tool for EditTool {
    fn name(&self) -> &str {
        "Edit"
    }
    fn description(&self) -> &str {
        "Replace old_string with new_string in a file"
    }
    fn input_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "path": {"type": "string"},
                "old_string": {"type": "string"},
                "new_string": {"type": "string"}
            },
            "required": ["path", "old_string", "new_string"]
        })
    }
    fn is_concurrency_safe(&self) -> bool {
        false
    }

    fn validate_input(&self, input: &Value) -> Result<(), ValidationError> {
        require_str_any(input, &["path", "file_path"])?;
        require_str(input, "old_string")?;
        if !input.get("new_string").is_some() {
            return Err(ValidationError::MissingField("new_string".into()));
        }
        Ok(())
    }

    async fn call(&self, input: Value, ctx: &ToolContext) -> Result<ToolOutput, ToolError> {
        let path = require_str_any(&input, &["path", "file_path"])?;
        let old_string = require_str(&input, "old_string")?.to_string();
        let new_string = input
            .get("new_string")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string();
        let full = ctx.resolve_path(&path);
        ctx.validate_write_root(&full)?;
        ctx.validate_plan_mode_write_path(&path)?;
        require_read_before_write(ctx, &full, &path)?;
        let content = blocking::read_to_string(full.clone()).await?;
        if !content.contains(&old_string) {
            return Err(ToolError::Execution(format!(
                "old_string not found in {}",
                full.display()
            )));
        }
        let updated = content.replacen(&old_string, &new_string, 1);
        blocking::write(full.clone(), updated).await?;
        Ok(ToolOutput {
            content: format!("Edited {}", full.display()),
            is_error: false,
        })
    }
}
