use super::super::{require_str, Tool, ToolContext, ToolError, ToolOutput, ValidationError};
use async_trait::async_trait;
use serde_json::{json, Value};
use std::process::Stdio;
use tokio::process::Command;

pub struct BashTool;

#[async_trait]
impl Tool for BashTool {
    fn name(&self) -> &str {
        "Bash"
    }
    fn description(&self) -> &str {
        "Run a shell command in the project root (word count, file listing, etc.)"
    }
    fn input_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "command": {"type": "string"},
                "description": {"type": "string"}
            },
            "required": ["command"]
        })
    }
    fn is_concurrency_safe(&self) -> bool {
        false
    }

    fn errors_abort_siblings(&self) -> bool {
        true
    }

    fn validate_input(&self, input: &Value) -> Result<(), ValidationError> {
        require_str(input, "command")?;
        Ok(())
    }

    async fn call(&self, input: Value, ctx: &ToolContext) -> Result<ToolOutput, ToolError> {
        let command = require_str(&input, "command")?;
        if command.contains("rm -rf") || command.contains("del /f") {
            return Err(ToolError::PermissionDenied(
                "destructive bash commands are blocked".into(),
            ));
        }
        let output = if cfg!(windows) {
            Command::new("cmd")
                .args(["/C", &command])
                .current_dir(&ctx.project_root)
                .stdout(Stdio::piped())
                .stderr(Stdio::piped())
                .output()
                .await
        } else {
            Command::new("sh")
                .args(["-c", &command])
                .current_dir(&ctx.project_root)
                .stdout(Stdio::piped())
                .stderr(Stdio::piped())
                .output()
                .await
        }
        .map_err(|e| ToolError::Execution(e.to_string()))?;
        let stdout = String::from_utf8_lossy(&output.stdout);
        let stderr = String::from_utf8_lossy(&output.stderr);
        let content = if stderr.is_empty() {
            stdout.to_string()
        } else {
            format!("{stdout}\n{stderr}")
        };
        Ok(ToolOutput {
            content,
            is_error: !output.status.success(),
        })
    }
}
