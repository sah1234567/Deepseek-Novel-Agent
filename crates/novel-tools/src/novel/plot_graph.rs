use crate::{require_str, Tool, ToolContext, ToolError, ToolOutput};
use async_trait::async_trait;
use novel_knowledge::{parse_causality_markdown, KnowledgeStore};
use serde_json::{json, Value};

pub struct PlotGraphTool;

#[async_trait]
impl Tool for PlotGraphTool {
    fn name(&self) -> &str {
        "PlotGraph"
    }
    fn description(&self) -> &str {
        "Traverse causality graph forward/backward"
    }
    fn input_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "event": {"type": "string"},
                "direction": {"type": "string", "enum": ["forward", "backward", "both"]},
                "depth": {"type": "integer"}
            },
            "required": ["event"]
        })
    }
    fn is_read_only(&self) -> bool {
        true
    }

    async fn call(&self, input: Value, ctx: &ToolContext) -> Result<ToolOutput, ToolError> {
        let event = require_str(&input, "event")?;
        let direction = input
            .get("direction")
            .and_then(|v| v.as_str())
            .unwrap_or("both");
        let depth = input.get("depth").and_then(|v| v.as_u64()).unwrap_or(3) as usize;
        let store = KnowledgeStore::new(&ctx.project_root);
        let path = "knowledge/plot/因果链.md";
        let content = store.read_file(path).unwrap_or_default();
        let graph = parse_causality_markdown(&content);
        let mut lines = Vec::new();
        if direction == "forward" || direction == "both" {
            for n in graph.traverse_forward(&event, depth) {
                lines.push(format!(
                    "forward: {} ({}) - {}",
                    n.id, n.chapter, n.description
                ));
            }
        }
        if direction == "backward" || direction == "both" {
            for n in graph.traverse_backward(&event, depth) {
                lines.push(format!(
                    "backward: {} ({}) - {}",
                    n.id, n.chapter, n.description
                ));
            }
        }
        Ok(ToolOutput {
            content: lines.join("\n"),
            is_error: false,
        })
    }
}
