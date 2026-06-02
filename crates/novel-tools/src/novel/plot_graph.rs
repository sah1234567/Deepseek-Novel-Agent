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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{PermissionMode, ToolContext};
    use tempfile::TempDir;

    #[tokio::test(flavor = "current_thread")]
    async fn plot_graph_traverses_causality_file() {
        let tmp = TempDir::new().unwrap();
        std::fs::create_dir_all(tmp.path().join("knowledge/plot")).unwrap();
        std::fs::write(
            tmp.path().join("knowledge/plot/因果链.md"),
            "| 章节 | 因 | 关系 | 果 | 描述 |\n\
             |------|-----|------|-----|------|\n\
             | Ch5 | E05 | 导致 | E06 | 误入禁地 |\n\
             | Ch5 | E06 | 触发 | E07 | 发现石碑 |\n",
        )
        .unwrap();
        let ctx = ToolContext {
            permission_mode: PermissionMode::Auto,
            project_root: tmp.path().to_path_buf(),
            ..ToolContext::new(tmp.path().to_path_buf())
        };
        let out = PlotGraphTool
            .call(
                json!({"event": "E05", "direction": "forward", "depth": 2}),
                &ctx,
            )
            .await
            .unwrap();
        assert!(out.content.contains("forward:"));
        assert!(out.content.contains("E06"));
    }
}
