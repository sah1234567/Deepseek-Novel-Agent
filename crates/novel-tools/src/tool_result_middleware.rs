//! Success-path middleware: append LLM-facing hints after tool execution.

use crate::paths::{normalize_rel_path, optional_file_path};
use crate::read_cache::{format_read_dedup_hint_from_input, is_read_dedup_stub};
use serde_json::Value;

pub(crate) struct MiddlewareCtx<'a> {
    pub tool_name: &'a str,
    pub tool_input: Option<&'a Value>,
    pub content: &'a str,
}

pub trait ToolResultMiddleware: Sync {
    fn append_lines(&self, ctx: &MiddlewareCtx<'_>) -> Vec<String>;
}

struct WriteEditFactMiddleware;

impl ToolResultMiddleware for WriteEditFactMiddleware {
    fn append_lines(&self, ctx: &MiddlewareCtx<'_>) -> Vec<String> {
        if ctx.tool_name != "Write" && ctx.tool_name != "Edit" {
            return Vec::new();
        }
        let Some(path) = ctx.tool_input.and_then(optional_file_path) else {
            return Vec::new();
        };
        let norm = normalize_rel_path(&path);
        vec![
            format!("[fact] touched: {norm}"),
            "[fact] context: session cache updated; conversation still has pre-edit text until you Read/Tail the changed range once.".into(),
        ]
    }
}

struct ReadDedupHintMiddleware;

impl ToolResultMiddleware for ReadDedupHintMiddleware {
    fn append_lines(&self, ctx: &MiddlewareCtx<'_>) -> Vec<String> {
        if ctx.tool_name != "Read" && ctx.tool_name != "Tail" {
            return Vec::new();
        }
        if !is_read_dedup_stub(ctx.content) {
            return Vec::new();
        }
        format_read_dedup_hint_from_input(ctx.tool_name, ctx.tool_input.unwrap_or(&Value::Null))
            .into_iter()
            .collect()
    }
}

static WRITE_EDIT_FACT: WriteEditFactMiddleware = WriteEditFactMiddleware;
static READ_DEDUP_HINT: ReadDedupHintMiddleware = ReadDedupHintMiddleware;

static SUCCESS_CHAIN: &[&dyn ToolResultMiddleware] = &[&WRITE_EDIT_FACT, &READ_DEDUP_HINT];

/// Collect append-only lines from all success middleware (in registration order).
pub(crate) fn append_middleware_lines(ctx: &MiddlewareCtx<'_>) -> Vec<String> {
    let mut lines = Vec::new();
    for middleware in SUCCESS_CHAIN {
        lines.extend(middleware.append_lines(ctx));
    }
    lines
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::FILE_UNCHANGED_STUB;
    use serde_json::json;

    #[test]
    fn write_edit_fact_middleware() {
        let input = json!({"file_path": "chapters/ch01.md"});
        let ctx = MiddlewareCtx {
            tool_name: "Write",
            tool_input: Some(&input),
            content: "Wrote file",
        };
        let lines = append_middleware_lines(&ctx);
        assert!(lines
            .iter()
            .any(|l| l.contains("[fact] touched: chapters/ch01.md")));
        assert!(lines.iter().any(|l| l.contains("[fact] context:")));
    }

    #[test]
    fn read_dedup_hint_middleware() {
        let input = json!({"file_path": "chapters/ch01.md", "offset": 5, "limit": 8});
        let ctx = MiddlewareCtx {
            tool_name: "Read",
            tool_input: Some(&input),
            content: FILE_UNCHANGED_STUB,
        };
        let lines = append_middleware_lines(&ctx);
        assert!(lines.iter().any(|l| l.contains("[read-dedup]")));
        assert!(lines.iter().any(|l| l.contains("offset:5 limit:8")));
    }

    #[test]
    fn read_without_dedup_stub_gets_no_hint() {
        let input = json!({"file_path": "a.md"});
        let ctx = MiddlewareCtx {
            tool_name: "Read",
            tool_input: Some(&input),
            content: "1\thello",
        };
        assert!(append_middleware_lines(&ctx).is_empty());
    }
}
