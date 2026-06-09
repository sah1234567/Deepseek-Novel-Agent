use crate::{Tool, ToolContext, ToolError, ToolOutput};
use async_trait::async_trait;
use novel_state::{SessionTodo, StateError};
use serde_json::{json, Value};

pub struct TodoWriteTool;

fn parse_todos(input: &Value) -> Result<Vec<SessionTodo>, ToolError> {
    let arr = input
        .get("todos")
        .and_then(|v| v.as_array())
        .ok_or_else(|| {
            ToolError::Validation(crate::ValidationError::MissingField("todos".into()))
        })?;
    let mut out = Vec::with_capacity(arr.len());
    for item in arr {
        let id = item
            .get("id")
            .and_then(|v| v.as_str())
            .ok_or_else(|| {
                ToolError::Validation(crate::ValidationError::MissingField("id".into()))
            })?
            .to_string();
        let content = item
            .get("content")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string();
        let status = item
            .get("status")
            .and_then(|v| v.as_str())
            .unwrap_or("pending")
            .to_string();
        out.push(SessionTodo {
            id,
            content,
            status,
        });
    }
    Ok(out)
}

#[async_trait]
impl Tool for TodoWriteTool {
    fn name(&self) -> &str {
        "TodoWrite"
    }
    fn description(&self) -> &str {
        "Track chapter and planning progress todos for the session"
    }
    fn input_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "todos": {
                    "type": "array",
                    "items": {
                        "type": "object",
                        "properties": {
                            "id": {"type": "string"},
                            "content": {"type": "string"},
                            "status": {"type": "string", "enum": ["pending", "in_progress", "completed", "cancelled"]}
                        },
                        "required": ["id", "content", "status"]
                    }
                },
                "merge": {"type": "boolean"}
            },
            "required": ["todos"]
        })
    }
    fn is_read_only(&self) -> bool {
        false
    }

    fn is_always_allowed(&self) -> bool {
        true
    }

    fn allowed_in_plan_mode(&self) -> bool {
        true
    }

    async fn call(&self, input: Value, ctx: &ToolContext) -> Result<ToolOutput, ToolError> {
        let todos = parse_todos(&input)?;
        let merge = input.get("merge").and_then(|v| v.as_bool()).unwrap_or(true);
        let db = ctx
            .db
            .as_ref()
            .ok_or_else(|| ToolError::Execution("database not available".into()))?;
        db.upsert_session_todos(&ctx.session_id, &todos, merge)
            .map_err(|e: StateError| ToolError::Execution(e.to_string()))?;
        Ok(ToolOutput {
            content: json!({"ok": true, "count": todos.len()}).to_string(),
            is_error: false,
        })
    }
}
