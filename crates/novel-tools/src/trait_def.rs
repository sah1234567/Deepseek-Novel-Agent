use crate::abort::InterruptBehavior;
use crate::{PermissionMode, PermissionResult, ToolContext, ValidationError};
use async_trait::async_trait;
use serde_json::Value;

#[derive(Debug, Clone)]
pub struct ToolOutput {
    pub content: String,
    pub is_error: bool,
}

#[async_trait]
pub trait Tool: Send + Sync {
    fn name(&self) -> &str;
    fn description(&self) -> &str;
    fn usage_hint(&self) -> &str {
        ""
    }
    fn input_schema(&self) -> Value;
    fn is_read_only(&self) -> bool {
        false
    }
    fn is_concurrency_safe(&self) -> bool {
        true
    }

    fn interrupt_behavior(&self) -> InterruptBehavior {
        if self.is_read_only() {
            InterruptBehavior::Cancel
        } else {
            InterruptBehavior::Block
        }
    }

    fn validate_input(&self, input: &Value) -> Result<(), ValidationError> {
        let _ = input;
        Ok(())
    }

    fn check_permissions(&self, input: &Value, ctx: &ToolContext) -> PermissionResult {
        if let Some(reason) = ctx.deny_rule_block(self.name(), tool_target_path(input)) {
            return PermissionResult::Deny { reason };
        }
        if self.name() == "ForkSubAgent" && !ctx.allow_fork {
            return PermissionResult::Deny {
                reason: "子 Agent 禁止嵌套 fork（sub_agent_running）".into(),
            };
        }
        if ctx.is_tool_always_allowed(self.name()) {
            return PermissionResult::Allow;
        }
        // Session todo list — not a filesystem write; should not block on Normal-mode approval.
        if self.name() == "TodoWrite" {
            return PermissionResult::Allow;
        }
        let mode = ctx.effective_permission_mode();
        if matches!(mode, PermissionMode::Plan) {
            if self.is_read_only() {
                return PermissionResult::Allow;
            }
            if matches!(self.name(), "Write" | "Edit") {
                if let Some(p) = tool_target_path(input) {
                    if ToolContext::is_under_plan_dir(p) {
                        return PermissionResult::Allow;
                    }
                }
                return PermissionResult::Deny {
                    reason: "plan mode: Write/Edit only allowed under plan/".into(),
                };
            }
            if matches!(
                self.name(),
                "TodoWrite" | "WebSearch" | "AskUserQuestion" | "InvokeSkill"
            ) {
                return PermissionResult::Allow;
            }
            return PermissionResult::Deny {
                reason: format!(
                    "plan mode: {} not available — use plan/ for drafts or switch permission mode",
                    self.name()
                ),
            };
        }
        match mode {
            PermissionMode::Auto | PermissionMode::Unattended => PermissionResult::Allow,
            _ if self.is_read_only() => PermissionResult::Allow,
            _ => PermissionResult::Ask {
                tool_name: self.name().into(),
                summary: self.get_summary(input),
            },
        }
    }

    async fn call(&self, input: Value, ctx: &ToolContext) -> Result<ToolOutput, super::ToolError>;

    fn get_summary(&self, input: &Value) -> String {
        format!("{} {:?}", self.name(), input)
    }
}

fn tool_target_path(input: &Value) -> Option<&str> {
    input
        .get("path")
        .or_else(|| input.get("file_path"))
        .and_then(|v| v.as_str())
}
