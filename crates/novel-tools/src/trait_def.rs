use crate::abort::InterruptBehavior;
use crate::{PermissionResult, ToolContext, ValidationError};
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
        crate::permission::evaluate_tool_permissions(
            self.name(),
            self.permission_caps(),
            &self.get_summary(input),
            input,
            ctx,
        )
    }

    async fn call(&self, input: Value, ctx: &ToolContext) -> Result<ToolOutput, super::ToolError>;

    fn get_summary(&self, input: &Value) -> String {
        format!("{} {:?}", self.name(), input)
    }

    // -- Predicate methods (OCP: replace hardcoded tool-name matching) --

    fn permission_caps(&self) -> crate::permission::ToolPermissionCaps {
        crate::permission::ToolPermissionCaps {
            is_read_only: self.is_read_only(),
            blocks_nested_fork: self.blocks_nested_fork(),
            is_always_allowed: self.is_always_allowed(),
            can_write_outside_plan_dir: self.can_write_outside_plan_dir(),
            allowed_in_plan_mode: self.allowed_in_plan_mode(),
            skips_normal_permission_ask: self.skips_normal_permission_ask(),
        }
    }

    /// ForkSubAgent — reject when subagent is already running.
    fn blocks_nested_fork(&self) -> bool {
        false
    }

    /// TodoWrite — always allowed regardless of permission mode.
    fn is_always_allowed(&self) -> bool {
        false
    }

    /// Write/Edit — Plan mode allows writes only under plan/ directory.
    fn can_write_outside_plan_dir(&self) -> bool {
        false
    }

    /// Tools callable in Plan mode beyond the read-only baseline.
    fn allowed_in_plan_mode(&self) -> bool {
        false
    }

    /// Read — triggers skill-reference tracking after a successful Read result.
    fn tracks_skill_references(&self) -> bool {
        false
    }

    /// InvokeSkill — marks the tool as a skill invocation for turn-level tracking.
    fn is_skill_invocation(&self) -> bool {
        false
    }

    /// Bash — a Bash error should abort concurrent sibling tools.
    fn errors_abort_siblings(&self) -> bool {
        false
    }

    /// Foreground orchestration tools (e.g. ForkSubAgent) that queue work without file I/O.
    fn skips_normal_permission_ask(&self) -> bool {
        false
    }

    /// Read/Tail — extract the line span this tool input covers, for committed-span tracking.
    fn extract_read_span(&self, _input: &Value, _total_lines: usize) -> Option<(usize, usize)> {
        None
    }

    /// Read/Tail — `range=` segment for `[read-dedup]` hint text when output is a dedup stub.
    fn read_dedup_range_label(&self, _input: &Value) -> Option<String> {
        None
    }

    /// Tools that may emit read-dedup middleware hints (Read/Tail).
    fn supports_read_dedup_hint(&self) -> bool {
        false
    }

    /// Max output lines before read-economy rejects tool_result; `None` = no limit.
    fn max_output_lines(&self, _input: &Value) -> Option<usize> {
        None
    }

    /// Hint appended when `max_output_lines` is exceeded.
    fn output_limit_exceeded_hint(&self) -> &'static str {
        "Use Grep to locate, then Read offset/limit or Tail for file-end segments."
    }
}
