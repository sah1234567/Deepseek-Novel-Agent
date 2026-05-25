use crate::{require_str, Tool, ToolContext, ToolError, ToolOutput};
use async_trait::async_trait;
use novel_knowledge::{
    derive_character_snapshot, derive_foreshadow_categories, derive_relation_cross_index,
    rebuild_index, KnowledgeStore,
};
use serde_json::{json, Value};

pub struct KnowledgeDeriveTool;

#[async_trait]
impl Tool for KnowledgeDeriveTool {
    fn name(&self) -> &str {
        "KnowledgeDerive"
    }
    fn description(&self) -> &str {
        "Derive snapshots, foreshadow categories, relation index, rebuild INDEX.md, or compress evolution logs"
    }
    fn input_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "operation": {
                    "type": "string",
                    "enum": ["characterSnapshot", "foreshadowCategories", "relationIndex", "rebuildIndex", "compressLogs"]
                },
                "character_path": {"type": "string", "description": "Relative path for characterSnapshot, e.g. knowledge/characters/林若烟.md"},
                "tail_rows": {"type": "integer", "description": "Rows to keep when compressing evolution logs (default 5)", "minimum": 1, "maximum": 20}
            },
            "required": ["operation"]
        })
    }
    fn is_concurrency_safe(&self) -> bool {
        false
    }

    async fn call(&self, input: Value, ctx: &ToolContext) -> Result<ToolOutput, ToolError> {
        let operation = require_str(&input, "operation")?;
        let store = KnowledgeStore::new(&ctx.project_root);
        match operation.as_str() {
            "characterSnapshot" => {
                let rel = require_str(&input, "character_path")?;
                let content = store.read_file(&rel)?;
                let updated = derive_character_snapshot(&content)?;
                store.write_file(&rel, &updated)?;
                Ok(ToolOutput {
                    content: format!("Derived snapshot in {rel}"),
                    is_error: false,
                })
            }
            "foreshadowCategories" => {
                let cats = derive_foreshadow_categories(&store)?;
                Ok(ToolOutput {
                    content: serde_json::to_string_pretty(&cats)
                        .map_err(|e| ToolError::Internal(e.to_string()))?,
                    is_error: false,
                })
            }
            "relationIndex" => {
                let idx = derive_relation_cross_index(&store)?;
                store.write_file("knowledge/characters/_关系与称呼索引.md", &idx)?;
                Ok(ToolOutput {
                    content: "Updated knowledge/characters/_关系与称呼索引.md".into(),
                    is_error: false,
                })
            }
            "rebuildIndex" => {
                let idx = rebuild_index(&store)?;
                Ok(ToolOutput {
                    content: idx,
                    is_error: false,
                })
            }
            "compressLogs" => {
                let tail_rows = input
                    .get("tail_rows")
                    .and_then(|v| v.as_u64())
                    .unwrap_or(5)
                    .min(20) as usize;
                let report = novel_compaction::apply_level2_knowledge(&ctx.project_root, tail_rows)
                    .map_err(|e| ToolError::Internal(e.to_string()))?;
                Ok(ToolOutput {
                    content: format!(
                        "Compressed evolution logs: {} files modified, {} rows merged (tail={tail_rows})",
                        report.files_compressed, report.total_rows_merged
                    ),
                    is_error: false,
                })
            }
            _ => Err(ToolError::Execution(format!(
                "unknown operation: {operation}"
            ))),
        }
    }
}
