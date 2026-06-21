//! Memory write detection and fork subagent permission guard.
//!
//! Two concerns, one module:
//! - **Path guard**: is a tool call targeting `memory/`? (used by `subagent_gate`)
//! - **Write detection**: did any tool call write to `memory/`? (used by turn loop)

use serde_json::Value;

// ── Path utilities ──────────────────────────────────────────────

/// Whether a project-relative path is inside the `memory/` directory.
pub fn is_memory_rel_path(path: &str) -> bool {
    let normalized = path.replace('\\', "/");
    normalized.starts_with("memory/") || normalized == "memory"
}

/// Whether a Write/Edit tool call targets a path under `memory/`.
pub fn is_memory_write_tool(tool_name: &str, input: &Value) -> bool {
    matches!(tool_name, "Write" | "Edit")
        && input
            .get("file_path")
            .and_then(|v| v.as_str())
            .is_some_and(is_memory_rel_path)
}

/// Check if a tool name + path is allowed under the memory fork permissions.
pub fn memory_fork_can_use_tool(tool_name: &str, path_hint: Option<&str>) -> bool {
    match tool_name {
        "Read" | "Grep" | "Glob" => true,
        "Write" | "Edit" => path_hint.is_some_and(is_memory_rel_path),
        _ => false,
    }
}

// ── Write detection ─────────────────────────────────────────────

/// True when any of the provided tool calls is a Write/Edit targeting `memory/`.
///
/// Decoupled from `novel-core` types: accepts `(tool_name, tool_arguments)` pairs.
/// The caller extracts the relevant fields from its own message type.
pub fn has_memory_writes_since<'a>(
    tool_calls: impl IntoIterator<Item = (&'a str, &'a Value)>,
) -> bool {
    tool_calls
        .into_iter()
        .any(|(name, args)| is_memory_write_tool(name, args))
}

// ── Tests ───────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    mod path_guard {
        use super::*;
        use serde_json::json;

        #[test]
        fn detects_memory_paths() {
            assert!(is_memory_write_tool(
                "Write",
                &json!({"file_path": "memory/style/pacing.md", "content": "x"})
            ));
            assert!(!is_memory_write_tool(
                "Write",
                &json!({"file_path": "chapters/ch1.md", "content": "x"})
            ));
        }

        #[test]
        fn normalizes_backslashes() {
            assert!(is_memory_rel_path("memory\\style\\pacing.md"));
            assert!(!is_memory_rel_path("chapters\\ch1.md"));
        }

        #[test]
        fn allows_read_grep_glob() {
            assert!(memory_fork_can_use_tool("Read", Some("chapters/ch1.md")));
            assert!(memory_fork_can_use_tool("Grep", Some("knowledge/")));
            assert!(memory_fork_can_use_tool("Glob", None));
        }

        #[test]
        fn allows_write_edit_in_memory_dir() {
            assert!(memory_fork_can_use_tool(
                "Write",
                Some("memory/style/pacing.md")
            ));
            assert!(memory_fork_can_use_tool(
                "Edit",
                Some("memory/plot_decisions/cp.md")
            ));
            assert!(memory_fork_can_use_tool(
                "Write",
                Some("memory/preferences.md")
            ));
        }

        #[test]
        fn denies_write_edit_outside_memory() {
            assert!(!memory_fork_can_use_tool("Write", Some("chapters/ch1.md")));
            assert!(!memory_fork_can_use_tool(
                "Edit",
                Some("knowledge/INDEX.md")
            ));
            assert!(!memory_fork_can_use_tool("Write", None));
        }

        #[test]
        fn denies_other_tools() {
            assert!(!memory_fork_can_use_tool("ForkSubAgent", None));
            assert!(!memory_fork_can_use_tool("Bash", None));
            assert!(!memory_fork_can_use_tool("TodoWrite", None));
            assert!(!memory_fork_can_use_tool("AskUserQuestion", None));
        }
    }

    mod write_detect {
        use super::*;
        use serde_json::json;

        #[test]
        fn detects_write_to_memory_path() {
            let writes = &json!({"file_path": "memory/style/pacing.md", "content": "x"});
            assert!(has_memory_writes_since(vec![("Write", writes)]));
        }

        #[test]
        fn ignores_write_to_non_memory_path() {
            let writes = &json!({"file_path": "chapters/ch1.md", "content": "x"});
            assert!(!has_memory_writes_since(vec![("Write", writes)]));
        }

        #[test]
        fn ignores_non_write_tools() {
            let reads = &json!({"file_path": "memory/style/pacing.md"});
            assert!(!has_memory_writes_since(vec![("Read", reads)]));
        }

        #[test]
        fn empty_calls_returns_false() {
            assert!(!has_memory_writes_since(Vec::<(&str, &Value)>::new()));
        }
    }
}
