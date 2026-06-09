//! Unified pipeline: tool execution result → LLM-facing tool_result text.

use crate::read_economy::enforce_tool_output_limits;
use crate::tool_error_hints::enhance_tool_error_for_llm;
use crate::tool_result_middleware::{append_middleware_lines, MiddlewareCtx};
use crate::{ToolError, ToolOutput, ToolRegistry};
use serde_json::Value;

pub struct ToolResultSpec<'a> {
    pub tool_name: &'a str,
    pub tool_input: Option<&'a Value>,
}

pub struct FormattedToolResult {
    /// Final text for SQLite, next LLM turn, and UI events.
    pub content: String,
    /// Post-economy raw body for PostToolUse hook preview (before middleware append).
    pub hook_preview: String,
}

pub const NEEDS_USER_INPUT_STUB: &str = "等待用户回答问题后再继续。";

pub fn format_tool_result_for_llm(
    registry: &ToolRegistry,
    spec: ToolResultSpec<'_>,
    result: Result<ToolOutput, ToolError>,
) -> FormattedToolResult {
    match result {
        Err(e) => format_error(spec, &e),
        Ok(out) if out.is_error => format_error(spec, &ToolError::Execution(out.content)),
        Ok(out) => format_success(registry, spec, out),
    }
}

fn format_error(spec: ToolResultSpec<'_>, err: &ToolError) -> FormattedToolResult {
    let content = enhance_tool_error_for_llm(spec.tool_name, err, spec.tool_input);
    FormattedToolResult {
        hook_preview: content.clone(),
        content,
    }
}

fn format_success(
    registry: &ToolRegistry,
    spec: ToolResultSpec<'_>,
    out: ToolOutput,
) -> FormattedToolResult {
    let tool_input = spec.tool_input.unwrap_or(&Value::Null);
    match enforce_tool_output_limits(registry, spec.tool_name, tool_input, &out) {
        Err(e) => format_error(spec, &e),
        Ok(checked) => {
            let hook_preview = checked.content.clone();
            let ctx = MiddlewareCtx {
                registry,
                tool_name: spec.tool_name,
                tool_input: spec.tool_input,
                content: &hook_preview,
            };
            let appends = append_middleware_lines(&ctx);
            let content = if appends.is_empty() {
                hook_preview.clone()
            } else {
                let mut lines = vec![hook_preview.clone()];
                lines.extend(appends);
                lines.join("\n")
            };
            FormattedToolResult {
                content,
                hook_preview,
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::default_registry;
    use crate::FILE_UNCHANGED_STUB;

    fn write_spec() -> ToolResultSpec<'static> {
        ToolResultSpec {
            tool_name: "Write",
            tool_input: None,
        }
    }

    #[test]
    fn write_injects_fact_and_context() {
        let spec = ToolResultSpec {
            tool_name: "Write",
            tool_input: Some(&serde_json::json!({"file_path": "chapters/ch01.md"})),
        };
        let registry = default_registry();
        let out = format_tool_result_for_llm(
            &registry,
            spec,
            Ok(ToolOutput {
                content: "written".into(),
                is_error: false,
            }),
        );
        assert!(out.content.contains("written"));
        assert!(out.content.contains("[fact] touched: chapters/ch01.md"));
        assert!(out.content.contains("[fact] context:"));
        assert_eq!(out.hook_preview, "written");
    }

    #[test]
    fn read_dedup_injects_hint() {
        let spec = ToolResultSpec {
            tool_name: "Read",
            tool_input: Some(
                &serde_json::json!({"file_path": "chapters/ch01.md", "offset": 5, "limit": 8}),
            ),
        };
        let registry = default_registry();
        let out = format_tool_result_for_llm(
            &registry,
            spec,
            Ok(ToolOutput {
                content: FILE_UNCHANGED_STUB.into(),
                is_error: false,
            }),
        );
        assert!(out.content.contains("[read-dedup]"));
        assert!(out.content.contains("offset:5 limit:8"));
    }

    #[test]
    fn permission_denied_has_error_prefix() {
        let registry = default_registry();
        let out = format_tool_result_for_llm(
            &registry,
            write_spec(),
            Err(ToolError::PermissionDenied("denied".into())),
        );
        assert_eq!(out.content, "Error: Permission denied: denied");
    }

    #[test]
    fn edit_execution_error_gets_next_steps() {
        let spec = ToolResultSpec {
            tool_name: "Edit",
            tool_input: None,
        };
        let err =
            ToolError::Execution("Read foo.md before editing (read-before-write policy)".into());
        let registry = default_registry();
        let out = format_tool_result_for_llm(&registry, spec, Err(err));
        assert!(out.content.contains("Next steps:"));
        assert!(out.content.contains("Read or Tail"));
    }

    #[test]
    fn soft_error_gets_hints() {
        let spec = ToolResultSpec {
            tool_name: "Edit",
            tool_input: None,
        };
        let registry = default_registry();
        let out = format_tool_result_for_llm(
            &registry,
            spec,
            Ok(ToolOutput {
                content: "Read foo.md before editing (read-before-write policy)".into(),
                is_error: true,
            }),
        );
        assert!(out.content.contains("Next steps:"));
    }

    #[test]
    fn economy_failure_gets_hints() {
        let mut content = String::new();
        for i in 0..100 {
            content.push_str(&format!("line {i}\n"));
        }
        let spec = ToolResultSpec {
            tool_name: "Grep",
            tool_input: Some(&serde_json::json!({"pattern": "x"})),
        };
        let registry = default_registry();
        let out = format_tool_result_for_llm(
            &registry,
            spec,
            Ok(ToolOutput {
                content,
                is_error: false,
            }),
        );
        assert!(out.content.contains("Read economy:"));
        assert!(out.content.contains("Next steps:"));
    }
}
