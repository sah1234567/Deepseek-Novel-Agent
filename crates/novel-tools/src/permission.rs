//! Permission evaluation engine extracted from the `Tool` trait's default impl.
//! Takes decomposed predicate values so it can be called from both the trait default
//! and from external code without needing a `&dyn Tool` object.

use crate::{optional_file_path, PermissionMode, PermissionResult, ToolContext};

#[allow(clippy::too_many_arguments)]
pub fn evaluate_tool_permissions(
    tool_name: &str,
    is_read_only: bool,
    blocks_nested_fork: bool,
    is_always_allowed: bool,
    can_write_outside_plan_dir: bool,
    allowed_in_plan_mode: bool,
    summary: &str,
    input: &serde_json::Value,
    ctx: &ToolContext,
) -> PermissionResult {
    if let Some(reason) = ctx.deny_rule_block(tool_name, optional_file_path(input).as_deref()) {
        return PermissionResult::Deny { reason };
    }
    if blocks_nested_fork && !ctx.allow_fork {
        return PermissionResult::Deny {
            reason: "子 Agent 禁止嵌套 fork（sub_agent_running）".into(),
        };
    }
    if ctx.is_tool_always_allowed(tool_name) {
        return PermissionResult::Allow;
    }
    if is_always_allowed {
        return PermissionResult::Allow;
    }
    let mode = ctx.effective_permission_mode();
    if matches!(mode, PermissionMode::Plan) {
        if is_read_only {
            return PermissionResult::Allow;
        }
        if can_write_outside_plan_dir {
            if let Some(p) = optional_file_path(input) {
                if ToolContext::is_under_plan_dir(&p) {
                    return PermissionResult::Allow;
                }
            }
            return PermissionResult::Deny {
                reason: "plan mode: Write/Edit only allowed under plan/".into(),
            };
        }
        if allowed_in_plan_mode {
            return PermissionResult::Allow;
        }
        return PermissionResult::Deny {
            reason: format!(
                "plan mode: {tool_name} not available — use plan/ for drafts or switch permission mode"
            ),
        };
    }
    match mode {
        PermissionMode::Auto | PermissionMode::Unattended => PermissionResult::Allow,
        _ if is_read_only => PermissionResult::Allow,
        _ => PermissionResult::Ask {
            tool_name: tool_name.into(),
            summary: summary.into(),
        },
    }
}
