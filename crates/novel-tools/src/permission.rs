//! Permission evaluation engine extracted from the `Tool` trait's default impl.
//! Takes decomposed predicate values so it can be called from both the trait default
//! and from external code without needing a `&dyn Tool` object.

use crate::{optional_file_path, PermissionMode, PermissionResult, ToolContext};

/// Tool-specific permission predicates (from `Tool` trait hooks).
#[derive(Debug, Clone, Copy)]
pub struct ToolPermissionCaps {
    pub is_read_only: bool,
    pub blocks_nested_fork: bool,
    pub is_always_allowed: bool,
    pub can_write_outside_plan_dir: bool,
    pub allowed_in_plan_mode: bool,
    pub skips_normal_permission_ask: bool,
}

pub fn evaluate_tool_permissions(
    tool_name: &str,
    caps: ToolPermissionCaps,
    summary: &str,
    input: &serde_json::Value,
    ctx: &ToolContext,
) -> PermissionResult {
    if let Some(reason) = ctx.deny_rule_block(tool_name, optional_file_path(input).as_deref()) {
        return PermissionResult::Deny { reason };
    }
    if caps.blocks_nested_fork && !ctx.allow_fork {
        return PermissionResult::Deny {
            reason: "子 Agent 禁止嵌套 fork（sub_agent_running）".into(),
        };
    }
    if ctx.is_tool_always_allowed(tool_name) || caps.is_always_allowed {
        return PermissionResult::Allow;
    }
    let mode = ctx.effective_permission_mode();
    if caps.skips_normal_permission_ask && matches!(mode, PermissionMode::Normal) {
        return PermissionResult::Allow;
    }
    if matches!(mode, PermissionMode::Plan) {
        return evaluate_plan_mode(
            tool_name,
            caps.is_read_only,
            caps.can_write_outside_plan_dir,
            caps.allowed_in_plan_mode,
            input,
        );
    }
    evaluate_standard_mode(mode, caps.is_read_only, tool_name, summary)
}

fn evaluate_plan_mode(
    tool_name: &str,
    is_read_only: bool,
    can_write_outside_plan_dir: bool,
    allowed_in_plan_mode: bool,
    input: &serde_json::Value,
) -> PermissionResult {
    if is_read_only {
        return PermissionResult::Allow;
    }
    if can_write_outside_plan_dir {
        return match optional_file_path(input) {
            Some(p) if ToolContext::is_under_plan_dir(&p) => PermissionResult::Allow,
            _ => PermissionResult::Deny {
                reason: "plan mode: Write/Edit only allowed under plan/".into(),
            },
        };
    }
    if allowed_in_plan_mode {
        return PermissionResult::Allow;
    }
    PermissionResult::Deny {
        reason: format!(
            "plan mode: {tool_name} not available — use plan/ for drafts or switch permission mode"
        ),
    }
}

fn evaluate_standard_mode(
    mode: PermissionMode,
    is_read_only: bool,
    tool_name: &str,
    summary: &str,
) -> PermissionResult {
    match mode {
        PermissionMode::Auto | PermissionMode::Unattended => PermissionResult::Allow,
        _ if is_read_only => PermissionResult::Allow,
        _ => PermissionResult::Ask {
            tool_name: tool_name.into(),
            summary: summary.into(),
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use tempfile::TempDir;

    fn ctx(mode: PermissionMode) -> ToolContext {
        let tmp = TempDir::new().unwrap();
        ToolContext {
            permission_mode: mode,
            project_root: tmp.path().to_path_buf(),
            ..ToolContext::new(tmp.path().to_path_buf())
        }
    }

    fn caps(
        is_read_only: bool,
        blocks_nested_fork: bool,
        is_always_allowed: bool,
        can_write_outside_plan_dir: bool,
        allowed_in_plan_mode: bool,
        skips_normal_permission_ask: bool,
    ) -> ToolPermissionCaps {
        ToolPermissionCaps {
            is_read_only,
            blocks_nested_fork,
            is_always_allowed,
            can_write_outside_plan_dir,
            allowed_in_plan_mode,
            skips_normal_permission_ask,
        }
    }

    #[test]
    fn deny_rule_blocks_before_mode_check() {
        let mut c = ctx(PermissionMode::Auto);
        c.deny_rules = vec!["Write".into()];
        let r = evaluate_tool_permissions(
            "Write",
            caps(false, false, false, true, false, false),
            "write",
            &json!({"file_path": "a.md"}),
            &c,
        );
        assert!(matches!(r, PermissionResult::Deny { .. }));
    }

    #[test]
    fn nested_fork_blocked_when_not_allowed() {
        let mut c = ctx(PermissionMode::Auto);
        c.allow_fork = false;
        let r = evaluate_tool_permissions(
            "ForkSubAgent",
            caps(true, true, false, false, false, false),
            "fork",
            &json!({}),
            &c,
        );
        assert!(matches!(r, PermissionResult::Deny { .. }));
    }

    #[test]
    fn plan_mode_write_only_under_plan() {
        let c = ctx(PermissionMode::Plan);
        let ok = evaluate_tool_permissions(
            "Write",
            caps(false, false, false, true, false, false),
            "write",
            &json!({"file_path": "plan/draft.md"}),
            &c,
        );
        assert!(matches!(ok, PermissionResult::Allow));
        let bad = evaluate_tool_permissions(
            "Write",
            caps(false, false, false, true, false, false),
            "write",
            &json!({"file_path": "chapters/ch01.md"}),
            &c,
        );
        assert!(matches!(bad, PermissionResult::Deny { .. }));
    }

    #[test]
    fn normal_mode_asks_for_write() {
        let c = ctx(PermissionMode::Normal);
        let r = evaluate_tool_permissions(
            "Write",
            caps(false, false, false, true, false, false),
            "write file",
            &json!({"file_path": "a.md"}),
            &c,
        );
        assert!(matches!(r, PermissionResult::Ask { .. }));
    }

    #[test]
    fn read_only_allowed_in_normal_mode() {
        let c = ctx(PermissionMode::Normal);
        let r = evaluate_tool_permissions(
            "Read",
            caps(true, false, false, false, false, false),
            "read",
            &json!({"file_path": "a.md"}),
            &c,
        );
        assert!(matches!(r, PermissionResult::Allow));
    }

    #[test]
    fn skips_normal_ask_when_predicate_set() {
        let c = ctx(PermissionMode::Normal);
        let r = evaluate_tool_permissions(
            "AskUserQuestion",
            caps(false, false, false, false, false, true),
            "question",
            &json!({}),
            &c,
        );
        assert!(matches!(r, PermissionResult::Allow));
    }
}
