use crate::{Tool, ToolContext, ToolError, ToolOutput};
use async_trait::async_trait;
use novel_state::{SessionTodo, StateError, TodoValidationError};
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
        "Maintain the author's session todo list. Two modes (replace flag): \
         replace=true — new batch: send the full list for this phase (first plan, after the \
         current batch is all completed/cancelled, or when the user replans); overwrites any \
         existing rows. \
         replace=false (default) — status update only: send items you changed this turn, using \
         ids from the current batch; unknown ids are skipped (not written) and the result \
         includes a warning to use replace=true for new tasks. \
         At most one in_progress; stable ids within a batch. \
         Mark every item completed or cancelled before you finish the turn; \
         start the next phase with replace=true."
    }

    fn input_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "todos": {
                    "type": "array",
                    "description": "replace=true: every item in the new batch. \
        replace=false: only changed items; ids must already exist—unknown ids are skipped with a warning.",
                    "items": {
                        "type": "object",
                        "properties": {
                            "id": { "type": "string", "description": "Stable id within the current batch" },
                            "content": { "type": "string" },
                            "status": { "type": "string", "enum": ["pending", "in_progress", "completed", "cancelled"] }
                        },
                        "required": ["id", "content", "status"]
                    }
                },
                "replace": {
                    "type": "boolean",
                    "description": "true: new batch (full list replaces the current plan). \
        false (default): update status on existing items only; unknown ids skipped with warning."
                }
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
        let replace = input
            .get("replace")
            .and_then(|v| v.as_bool())
            .unwrap_or(false);
        let db = ctx
            .db
            .as_ref()
            .ok_or_else(|| ToolError::Execution("database not available".into()))?;

        let existing = db
            .list_session_todos(&ctx.session_id)
            .map_err(|e: StateError| ToolError::Execution(e.to_string()))?;

        let (to_apply, skipped_ids) = if replace {
            (todos, Vec::new())
        } else {
            novel_state::partition_status_updates(&existing, &todos)
        };

        let warning = if skipped_ids.is_empty() {
            None
        } else {
            Some(format!(
                "以下 id 不在当前待办列表中，已跳过：{}。若需新建任务请使用 replace=true。",
                skipped_ids.join(", ")
            ))
        };

        if !to_apply.is_empty() {
            novel_state::validate_todo_upsert(&existing, &to_apply, replace).map_err(
                |e: TodoValidationError| {
                    ToolError::Validation(crate::ValidationError::InvalidField(e.to_string()))
                },
            )?;
            db.upsert_session_todos(&ctx.session_id, &to_apply, replace)
                .map_err(|e: StateError| ToolError::Execution(e.to_string()))?;
        }

        let result = if let Some(ref w) = warning {
            json!({"ok": true, "count": to_apply.len(), "warning": w})
        } else {
            json!({"ok": true, "count": to_apply.len()})
        };

        Ok(ToolOutput {
            content: result.to_string(),
            is_error: false,
        })
    }
}
