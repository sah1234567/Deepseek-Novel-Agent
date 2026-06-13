//! Executor gate: subagents keep the same LLM tool schemas as the main agent (KV cache),
//! but mutating tools are rejected at call time when `subagent_queue` is absent.

use crate::ToolContext;

const SUBAGENT_MUTATOR_DENY: &str =
    "子 Agent 禁止 Write/Edit/TodoWrite。请将结论写入最终 assistant 报告正文；正典写盘由主 Agent 执行。";

/// When running inside a fork subagent (`subagent_queue` absent), block mutators.
/// Main session always wires `subagent_queue: Some` even when `allow_fork` is false
/// (nested-fork guard while other subagents drain).
pub fn subagent_mutator_gate(tool_name: &str, ctx: &ToolContext) -> Option<String> {
    if ctx.subagent_queue.is_some() {
        return None;
    }
    match tool_name {
        "Write" | "Edit" | "TodoWrite" => Some(SUBAGENT_MUTATOR_DENY.into()),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{PermissionMode, ToolContext};
    use std::path::PathBuf;
    use std::sync::Arc;

    fn ctx(subagent: bool) -> ToolContext {
        ToolContext {
            permission_mode: PermissionMode::Auto,
            allow_fork: !subagent,
            subagent_queue: if subagent {
                None
            } else {
                Some(Arc::new(std::sync::Mutex::new(Vec::new())))
            },
            ..ToolContext::new(PathBuf::from("."))
        }
    }

    #[test]
    fn subagent_gate_denies_write_edit_todo_when_not_allow_fork() {
        let c = ctx(true);
        assert!(subagent_mutator_gate("Write", &c).is_some());
        assert!(subagent_mutator_gate("Edit", &c).is_some());
        assert!(subagent_mutator_gate("TodoWrite", &c).is_some());
    }

    #[test]
    fn subagent_gate_allows_write_when_allow_fork() {
        let c = ctx(false);
        assert!(subagent_mutator_gate("Write", &c).is_none());
        assert!(subagent_mutator_gate("Edit", &c).is_none());
    }

    #[test]
    fn subagent_gate_allows_websearch_when_not_allow_fork() {
        let c = ctx(true);
        assert!(subagent_mutator_gate("WebSearch", &c).is_none());
        assert!(subagent_mutator_gate("Read", &c).is_none());
    }
}
