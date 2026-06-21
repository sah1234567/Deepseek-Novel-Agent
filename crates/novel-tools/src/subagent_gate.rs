//! Executor gate: subagents keep the same LLM tool schemas as the main agent (KV cache),
//! but mutating tools are rejected at call time when `subagent_queue` is absent.

use crate::{optional_file_path, ToolContext};
use novel_memory::{is_memory_write_tool, memory_fork_can_use_tool};
use serde_json::Value;

const SUBAGENT_MUTATOR_DENY: &str =
    "子 Agent 禁止 Write/Edit/TodoWrite。请将结论写入最终 assistant 报告正文；正典写盘由主 Agent 执行。";

const MEMORY_FORK_DENY: &str = "记忆提取子 Agent 仅允许 Read/Grep/Glob 及 memory/ 内 Write/Edit。";

/// When running inside a fork subagent (`subagent_queue` absent), block mutators.
/// Main session always wires `subagent_queue: Some` even when `allow_fork` is false
/// (nested-fork guard while other subagents drain).
///
/// Memory-extraction fork (`memory_fork_mode`): Read/Grep/Glob + memory/ Write/Edit only.
pub fn subagent_mutator_gate(
    tool_name: &str,
    ctx: &ToolContext,
    input: Option<&Value>,
) -> Option<String> {
    if ctx.subagent_queue.is_some() {
        return None;
    }
    if ctx.memory_fork_mode {
        let input = input.unwrap_or(&Value::Null);
        return if memory_fork_can_use_tool(tool_name, optional_file_path(input).as_deref()) {
            None
        } else if is_memory_write_tool(tool_name, input) {
            Some("Write/Edit 必须指定 memory/ 目录内的 file_path。".into())
        } else {
            Some(MEMORY_FORK_DENY.into())
        };
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
    use serde_json::json;
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

    fn memory_fork_ctx() -> ToolContext {
        ToolContext {
            memory_fork_mode: true,
            subagent_queue: None,
            allow_fork: false,
            ..ToolContext::new(PathBuf::from("."))
        }
    }

    #[test]
    fn subagent_gate_denies_write_edit_todo_when_not_allow_fork() {
        let c = ctx(true);
        assert!(subagent_mutator_gate("Write", &c, None).is_some());
        assert!(subagent_mutator_gate("Edit", &c, None).is_some());
        assert!(subagent_mutator_gate("TodoWrite", &c, None).is_some());
    }

    #[test]
    fn subagent_gate_allows_write_when_allow_fork() {
        let c = ctx(false);
        assert!(subagent_mutator_gate("Write", &c, None).is_none());
        assert!(subagent_mutator_gate("Edit", &c, None).is_none());
    }

    #[test]
    fn memory_fork_allows_memory_write() {
        let c = memory_fork_ctx();
        let input = json!({"file_path": "memory/style/pacing.md", "content": "x"});
        assert!(subagent_mutator_gate("Write", &c, Some(&input)).is_none());
    }

    #[test]
    fn memory_fork_denies_chapter_write() {
        let c = memory_fork_ctx();
        let input = json!({"file_path": "chapters/ch1.md", "content": "x"});
        assert!(subagent_mutator_gate("Write", &c, Some(&input)).is_some());
    }

    #[test]
    fn memory_fork_denies_bash() {
        let c = memory_fork_ctx();
        assert!(subagent_mutator_gate("Bash", &c, None).is_some());
    }
}
