use crate::{require_str, Tool, ToolContext, ToolError, ToolOutput};
use async_trait::async_trait;
use novel_knowledge::{
    list_pending, query_chapter, query_summary, AuditChapterRow, AuditStatusSummary,
    KnowledgeStore, PendingFilter,
};
use serde::Serialize;
use serde_json::{json, Value};

#[derive(Debug, Serialize)]
struct AuditStatusQueryResult {
    operation: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    summary: Option<AuditStatusSummary>,
    #[serde(skip_serializing_if = "Option::is_none")]
    chapter: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    row: Option<AuditChapterRow>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pending_chapters: Option<Vec<u32>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    audit_type: Option<String>,
}

fn parse_pending_filter(audit_type: &str) -> Result<PendingFilter, ToolError> {
    match audit_type {
        "pa" => Ok(PendingFilter::PlanPa),
        "ka" => Ok(PendingFilter::BodyKa),
        "cca" => Ok(PendingFilter::CraftCca),
        "any" => Ok(PendingFilter::Any),
        _ => Err(ToolError::Execution(format!(
            "unknown audit_type: {audit_type}. Use: pa, ka, cca, any"
        ))),
    }
}

pub(crate) fn run_audit_status_query(
    store: &KnowledgeStore,
    operation: &str,
    input: &Value,
) -> Result<ToolOutput, ToolError> {
    let result = match operation {
        "summary" => AuditStatusQueryResult {
            operation: operation.into(),
            summary: Some(query_summary(store).map_err(map_knowledge_err)?),
            chapter: None,
            row: None,
            pending_chapters: None,
            audit_type: None,
        },
        "chapter" => {
            let ch = input
                .get("chapter")
                .and_then(|v| v.as_u64())
                .ok_or_else(|| {
                    ToolError::Execution("chapter required for chapter operation".into())
                })? as u32;
            AuditStatusQueryResult {
                operation: operation.into(),
                summary: None,
                chapter: Some(ch),
                row: query_chapter(store, ch).map_err(map_knowledge_err)?,
                pending_chapters: None,
                audit_type: None,
            }
        }
        "pending" => {
            let audit_type = require_str(input, "audit_type")?;
            let filter = parse_pending_filter(&audit_type)?;
            AuditStatusQueryResult {
                operation: operation.into(),
                summary: None,
                chapter: None,
                row: None,
                pending_chapters: Some(list_pending(store, filter).map_err(map_knowledge_err)?),
                audit_type: Some(audit_type.to_string()),
            }
        }
        _ => {
            return Err(ToolError::Execution(format!(
                "unknown operation: {operation}. Use: summary, chapter, pending"
            )))
        }
    };

    Ok(ToolOutput {
        content: serde_json::to_string_pretty(&result)
            .map_err(|e| ToolError::Internal(e.to_string()))?,
        is_error: false,
    })
}

fn map_knowledge_err(e: novel_knowledge::KnowledgeError) -> ToolError {
    ToolError::Execution(e.to_string())
}

pub struct AuditStatusQueryTool;

#[async_trait]
impl Tool for AuditStatusQueryTool {
    fn name(&self) -> &str {
        "AuditStatusQuery"
    }
    fn description(&self) -> &str {
        "Query knowledge/meta/audit-status.md — audit pass state per chapter (PA/KA/CCA)"
    }
    fn input_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "operation": {
                    "type": "string",
                    "enum": ["summary", "chapter", "pending"],
                    "description": "summary=ledger overview, chapter=one row, pending=chapters not yet passed"
                },
                "chapter": {
                    "type": "integer",
                    "description": "Chapter number for chapter operation"
                },
                "audit_type": {
                    "type": "string",
                    "enum": ["pa", "ka", "cca", "any"],
                    "description": "Which audit column for pending operation"
                }
            },
            "required": ["operation"]
        })
    }
    fn is_read_only(&self) -> bool {
        true
    }

    async fn call(&self, input: Value, ctx: &ToolContext) -> Result<ToolOutput, ToolError> {
        let operation = require_str(&input, "operation")?;
        let store = KnowledgeStore::new(&ctx.project_root);
        run_audit_status_query(&store, &operation, &input)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::PermissionMode;
    use novel_knowledge::{mark_audited, AuditKind};
    use tempfile::TempDir;

    #[tokio::test]
    async fn summary_operation() {
        let tmp = TempDir::new().expect("tmpdir");
        let store = KnowledgeStore::new(tmp.path());
        mark_audited(&store, AuditKind::PlanAuditor, &[1], "task").expect("mark");
        let out = run_audit_status_query(&store, "summary", &json!({})).expect("query");
        assert!(out.content.contains("plan_pa_passed_through"));
    }

    #[tokio::test]
    async fn pending_pa_lists_chapter() {
        let tmp = TempDir::new().expect("tmpdir");
        let store = KnowledgeStore::new(tmp.path());
        mark_audited(&store, AuditKind::PlanAuditor, &[2], "t").expect("mark");
        let out =
            run_audit_status_query(&store, "pending", &json!({"audit_type": "pa"})).expect("query");
        assert!(out.content.contains("2"));
    }

    #[test]
    fn schema_has_snake_case_fields() {
        let tool = AuditStatusQueryTool;
        let schema = tool.input_schema();
        assert!(schema["properties"].get("audit_type").is_some());
        assert!(schema["properties"].get("operation").is_some());
        let _ = PermissionMode::Normal;
    }
}
