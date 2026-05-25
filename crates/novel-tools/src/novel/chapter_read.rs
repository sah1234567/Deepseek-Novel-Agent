use crate::{blocking, require_str, Tool, ToolContext, ToolError, ToolOutput};
use async_trait::async_trait;
use serde_json::{json, Value};

pub struct ChapterReadTool;

#[async_trait]
impl Tool for ChapterReadTool {
    fn name(&self) -> &str {
        "ChapterRead"
    }
    fn description(&self) -> &str {
        "Read chapter content. Prefer head/tail/range over full. \
         tail: previous-chapter hook for continuity; range: a specific segment; \
         full: only before write/edit or when a full-chapter audit is required."
    }
    fn input_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "chapter": {"type": "string"},
                "context": {"type": "string", "enum": ["full", "head", "tail", "range"]},
                "lines": {"type": "integer"},
                "start": {"type": "integer"},
                "end": {"type": "integer"}
            },
            "required": ["chapter"]
        })
    }
    fn is_read_only(&self) -> bool {
        true
    }

    async fn call(&self, input: Value, ctx: &ToolContext) -> Result<ToolOutput, ToolError> {
        let chapter = require_str(&input, "chapter")?;
        let mode = input
            .get("context")
            .and_then(|v| v.as_str())
            .unwrap_or("full");
        let lines_n = input.get("lines").and_then(|v| v.as_u64()).unwrap_or(100) as usize;
        let path = if chapter.contains('/') || chapter.ends_with(".md") {
            ctx.resolve_path(&chapter)
        } else {
            let num = chapter
                .trim_start_matches("chapter-")
                .trim_start_matches("Chapter ");
            ctx.project_root.join(format!(
                "chapters/chapter-{:0>3}.md",
                num.parse::<u32>().unwrap_or(0)
            ))
        };
        let content = blocking::read_to_string(path).await?;
        let lines: Vec<&str> = content.lines().collect();
        let slice = match mode {
            "head" => lines
                .iter()
                .take(lines_n)
                .copied()
                .collect::<Vec<_>>()
                .join("\n"),
            "tail" => lines
                .iter()
                .rev()
                .take(lines_n)
                .copied()
                .collect::<Vec<_>>()
                .into_iter()
                .rev()
                .collect::<Vec<_>>()
                .join("\n"),
            "range" => {
                let start = input.get("start").and_then(|v| v.as_u64()).unwrap_or(0) as usize;
                let end = input
                    .get("end")
                    .and_then(|v| v.as_u64())
                    .unwrap_or(lines.len() as u64) as usize;
                lines
                    .iter()
                    .skip(start)
                    .take(end.saturating_sub(start))
                    .copied()
                    .collect::<Vec<_>>()
                    .join("\n")
            }
            _ => content,
        };
        Ok(ToolOutput {
            content: slice,
            is_error: false,
        })
    }
}
