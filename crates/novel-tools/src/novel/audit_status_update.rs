use crate::{require_str, Tool, ToolContext, ToolError, ToolOutput};
use async_trait::async_trait;
use novel_graph::GraphTracker;
use novel_knowledge::{mark_audited, AuditKind, KnowledgeStore};
use serde_json::{json, Value};

fn parse_audit_kind(audit_type: &str) -> Result<AuditKind, ToolError> {
    match audit_type {
        "pa" => Ok(AuditKind::PlanAuditor),
        "ka" => Ok(AuditKind::KnowledgeAuditor),
        "cca" => Ok(AuditKind::ChapterCraftAnalyzer),
        _ => Err(ToolError::Execution(format!(
            "unknown audit_type: {audit_type}. Use: pa, ka, cca"
        ))),
    }
}

fn parse_chapters(input: &Value) -> Result<Vec<u32>, ToolError> {
    let arr = input
        .get("chapters")
        .and_then(|v| v.as_array())
        .ok_or_else(|| ToolError::Execution("chapters required (non-empty array)".into()))?;
    if arr.is_empty() {
        return Err(ToolError::Execution(
            "chapters required (non-empty array)".into(),
        ));
    }
    let mut out = Vec::with_capacity(arr.len());
    for v in arr {
        let ch = v
            .as_u64()
            .ok_or_else(|| ToolError::Execution("each chapter must be a positive integer".into()))?
            as u32;
        if ch == 0 {
            return Err(ToolError::Execution(
                "each chapter must be a positive integer".into(),
            ));
        }
        out.push(ch);
    }
    Ok(out)
}

fn maybe_mark_node_verifying(
    project_root: &std::path::Path,
    node_id: &str,
) -> Result<String, ToolError> {
    let mut t = GraphTracker::load(project_root)
        .map_err(|e| ToolError::Execution(e.to_string()))?
        .ok_or_else(|| {
            ToolError::Execution("no plan-graph.json — cannot mark node Verifying".into())
        })?;
    t.mark_verifying(node_id)
        .map_err(|e| ToolError::Execution(e.to_string()))?;
    t.save(project_root)
        .map_err(|e| ToolError::Execution(e.to_string()))?;
    Ok(format!("Node `{node_id}` → Verifying."))
}

pub struct AuditStatusUpdateTool;

#[async_trait]
impl Tool for AuditStatusUpdateTool {
    fn name(&self) -> &str {
        "AuditStatusUpdate"
    }
    fn description(&self) -> &str {
        "Mark chapters 已审计 in knowledge/meta/audit-status.md after audit-plan/knowledge/craft InvokeSkill"
    }
    fn input_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "audit_type": {
                    "type": "string",
                    "enum": ["pa", "ka", "cca"],
                    "description": "Which audit column to update"
                },
                "chapters": {
                    "type": "array",
                    "items": { "type": "integer" },
                    "description": "Chapter numbers (required, non-empty)"
                },
                "task_snippet": {
                    "type": "string",
                    "description": "Optional note stored in audit-status remark column"
                },
                "node_id": {
                    "type": "string",
                    "description": "Optional graph node to mark Verifying after ledger update"
                }
            },
            "required": ["audit_type", "chapters"]
        })
    }
    fn is_read_only(&self) -> bool {
        false
    }

    async fn call(&self, input: Value, ctx: &ToolContext) -> Result<ToolOutput, ToolError> {
        let audit_type = require_str(&input, "audit_type")?;
        let kind = parse_audit_kind(&audit_type)?;
        let chapters = parse_chapters(&input)?;
        let task_snippet = input
            .get("task_snippet")
            .and_then(|v| v.as_str())
            .unwrap_or("");
        let store = KnowledgeStore::new(&ctx.project_root);
        mark_audited(&store, kind, &chapters, task_snippet)
            .map_err(|e| ToolError::Execution(e.to_string()))?;

        let mut parts = vec![format!(
            "Marked {audit_type} 已审计 for chapters {:?}.",
            chapters
        )];
        if let Some(node_id) = input.get("node_id").and_then(|v| v.as_str()) {
            parts.push(maybe_mark_node_verifying(&ctx.project_root, node_id)?);
        }
        Ok(ToolOutput {
            content: parts.join(" "),
            is_error: false,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::PermissionMode;
    use novel_knowledge::query_chapter;
    use tempfile::TempDir;

    #[tokio::test]
    async fn marks_audited_fail_closed_on_empty_chapters() {
        let tmp = TempDir::new().expect("tmpdir");
        let ctx = ToolContext {
            permission_mode: PermissionMode::Auto,
            project_root: tmp.path().to_path_buf(),
            ..ToolContext::new(tmp.path().to_path_buf())
        };
        let err = AuditStatusUpdateTool
            .call(json!({"audit_type": "pa", "chapters": []}), &ctx)
            .await
            .unwrap_err();
        assert!(matches!(err, ToolError::Execution(_)));
    }

    #[tokio::test]
    async fn marks_audited_for_chapter() {
        let tmp = TempDir::new().expect("tmpdir");
        let ctx = ToolContext {
            permission_mode: PermissionMode::Auto,
            project_root: tmp.path().to_path_buf(),
            ..ToolContext::new(tmp.path().to_path_buf())
        };
        AuditStatusUpdateTool
            .call(
                json!({"audit_type": "pa", "chapters": [3], "task_snippet": "audit-plan"}),
                &ctx,
            )
            .await
            .expect("mark");
        let store = KnowledgeStore::new(tmp.path());
        let row = query_chapter(&store, 3).expect("row").expect("exists");
        assert_eq!(row.plan_pa, "已审计");
    }

    #[tokio::test]
    async fn marks_audited_with_node_verifying() {
        let tmp = TempDir::new().expect("tmpdir");
        let plan = novel_graph::PlanGraph {
            version: "1".into(),
            max_parallel_nodes: 4,
            nodes: vec![novel_graph::PlanNode {
                id: "world-bible".into(),
                title: "WB".into(),
                spec: Some("x".into()),
                tags: vec!["world_bible".into()],
                ..Default::default()
            }],
            ..Default::default()
        };
        novel_graph::save_plan(tmp.path(), &plan).unwrap();
        let mut t = novel_graph::GraphTracker::new(plan);
        t.state.nodes.get_mut("world-bible").unwrap().status = novel_graph::NodeStatus::Running;
        t.save(tmp.path()).unwrap();
        let ctx = ToolContext {
            permission_mode: PermissionMode::Auto,
            project_root: tmp.path().to_path_buf(),
            ..ToolContext::new(tmp.path().to_path_buf())
        };
        let out = AuditStatusUpdateTool
            .call(
                json!({
                    "audit_type": "ka",
                    "chapters": [1],
                    "node_id": "world-bible"
                }),
                &ctx,
            )
            .await
            .expect("mark");
        assert!(out.content.contains("Verifying"));
        assert!(parse_audit_kind("cca").is_ok());
        assert!(parse_audit_kind("nope").is_err());
        assert!(parse_chapters(&json!({"chapters":[0]})).is_err());
    }

    #[tokio::test]
    async fn node_verifying_requires_plan() {
        let tmp = TempDir::new().expect("tmpdir");
        let ctx = ToolContext {
            permission_mode: PermissionMode::Auto,
            project_root: tmp.path().to_path_buf(),
            ..ToolContext::new(tmp.path().to_path_buf())
        };
        let err = AuditStatusUpdateTool
            .call(
                json!({"audit_type": "cca", "chapters": [2], "node_id": "x"}),
                &ctx,
            )
            .await
            .unwrap_err();
        assert!(matches!(err, ToolError::Execution(_)));
    }
}
