//! Success-path middleware: append LLM-facing hints after tool execution.

use crate::paths::{normalize_rel_path, optional_file_path};
use crate::read_cache::{format_read_dedup_hint_from_input, is_read_dedup_stub};
use crate::ToolRegistry;
use serde_json::Value;

pub(crate) struct MiddlewareCtx<'a> {
    pub registry: &'a ToolRegistry,
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
        let context_line = if ctx.tool_name == "Edit" {
            "[fact] context: read cache updated for this path (partial patch or full file after replace_all); \
             conversation may still show pre-edit text — next Edit: build old_string from on-disk text (Grep if needed); \
             do not re-Read same offset/limit (dedup stub); use different offset/limit or rely on cache.".into()
        } else {
            "[fact] context: session cache holds full file; conversation may still show pre-write text — Tail/Read to confirm after large overwrites.".into()
        };
        vec![format!("[fact] touched: {norm}"), context_line]
    }
}

struct ReadDedupHintMiddleware;

impl ToolResultMiddleware for ReadDedupHintMiddleware {
    fn append_lines(&self, ctx: &MiddlewareCtx<'_>) -> Vec<String> {
        let Some(tool) = ctx.registry.get(ctx.tool_name) else {
            return Vec::new();
        };
        if !tool.supports_read_dedup_hint() {
            return Vec::new();
        }
        if !is_read_dedup_stub(ctx.content) {
            return Vec::new();
        }
        format_read_dedup_hint_from_input(
            ctx.registry,
            ctx.tool_name,
            ctx.tool_input.unwrap_or(&Value::Null),
        )
        .into_iter()
        .collect()
    }
}

static WRITE_EDIT_FACT: WriteEditFactMiddleware = WriteEditFactMiddleware;
static READ_DEDUP_HINT: ReadDedupHintMiddleware = ReadDedupHintMiddleware;

static SUCCESS_CHAIN: &[&dyn ToolResultMiddleware] = &[&WRITE_EDIT_FACT, &READ_DEDUP_HINT];

/// LLM-visible tool_result text when a tool_use was interrupted before completion.
pub fn format_interrupted_tool_result(tool_name: &str, _tool_call_id: &str) -> String {
    format!("[fact] 工具未完成：用户已中断会话，{tool_name} 无结果。")
}

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
    use crate::default_registry;
    use crate::FILE_UNCHANGED_STUB;
    use serde_json::json;

    #[test]
    fn write_edit_fact_middleware() {
        let registry = default_registry();
        let input = json!({"file_path": "chapters/ch01.md"});
        let ctx = MiddlewareCtx {
            registry: &registry,
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
        let registry = default_registry();
        let input = json!({"file_path": "chapters/ch01.md", "offset": 5, "limit": 8});
        let ctx = MiddlewareCtx {
            registry: &registry,
            tool_name: "Read",
            tool_input: Some(&input),
            content: FILE_UNCHANGED_STUB,
        };
        let lines = append_middleware_lines(&ctx);
        assert!(lines.iter().any(|l| l.contains("[read-dedup]")));
        assert!(lines.iter().any(|l| l.contains("offset:5 limit:8")));
    }

    #[test]
    fn edit_fact_mentions_updated_cache() {
        let registry = default_registry();
        let input = json!({"file_path": "chapters/ch01.md"});
        let ctx = MiddlewareCtx {
            registry: &registry,
            tool_name: "Edit",
            tool_input: Some(&input),
            content: "Edited file",
        };
        let lines = append_middleware_lines(&ctx);
        assert!(lines.iter().any(|l| l.contains("read cache updated")));
    }

    #[test]
    fn read_without_dedup_stub_gets_no_hint() {
        let registry = default_registry();
        let input = json!({"file_path": "a.md"});
        let ctx = MiddlewareCtx {
            registry: &registry,
            tool_name: "Read",
            tool_input: Some(&input),
            content: "1\thello",
        };
        assert!(append_middleware_lines(&ctx).is_empty());
    }
}
