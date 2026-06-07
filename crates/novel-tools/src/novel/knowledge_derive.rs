use crate::{require_str, Tool, ToolContext, ToolError, ToolOutput};
use async_trait::async_trait;
use novel_knowledge::{
    derive_character_snapshot, derive_foreshadow_categories, derive_relation_cross_index,
    rebuild_index, KnowledgeStore,
};
use serde_json::{json, Value};

pub struct KnowledgeDeriveTool;

pub(crate) fn derive_character_snapshot_op(
    store: &KnowledgeStore,
    input: &Value,
) -> Result<ToolOutput, ToolError> {
    let rel = require_str(input, "character_path")?;
    let content = store.read_file(&rel)?;
    let updated = derive_character_snapshot(&content)?;
    store.write_file(&rel, &updated)?;
    Ok(ToolOutput {
        content: format!("Derived snapshot in {rel}"),
        is_error: false,
    })
}

pub(crate) fn derive_foreshadow_categories_op(
    store: &KnowledgeStore,
) -> Result<ToolOutput, ToolError> {
    let cats = derive_foreshadow_categories(store)?;
    Ok(ToolOutput {
        content: serde_json::to_string_pretty(&cats)
            .map_err(|e| ToolError::Internal(e.to_string()))?,
        is_error: false,
    })
}

pub(crate) fn derive_relation_index_op(store: &KnowledgeStore) -> Result<ToolOutput, ToolError> {
    let idx = derive_relation_cross_index(store)?;
    store.write_file("knowledge/characters/_关系与称呼索引.md", &idx)?;
    Ok(ToolOutput {
        content: "Updated knowledge/characters/_关系与称呼索引.md".into(),
        is_error: false,
    })
}

pub(crate) fn rebuild_knowledge_index_op(store: &KnowledgeStore) -> Result<ToolOutput, ToolError> {
    let idx = rebuild_index(store)?;
    Ok(ToolOutput {
        content: idx,
        is_error: false,
    })
}

pub(crate) fn run_knowledge_derive_op(
    store: &KnowledgeStore,
    operation: &str,
    input: &Value,
) -> Result<ToolOutput, ToolError> {
    match operation {
        "character_snapshot" => derive_character_snapshot_op(store, input),
        "foreshadow_categories" => derive_foreshadow_categories_op(store),
        "relation_index" => derive_relation_index_op(store),
        "rebuild_index" => rebuild_knowledge_index_op(store),
        _ => Err(ToolError::Execution(format!(
            "unknown operation: {operation}"
        ))),
    }
}

#[async_trait]
impl Tool for KnowledgeDeriveTool {
    fn name(&self) -> &str {
        "KnowledgeDerive"
    }
    fn description(&self) -> &str {
        "Derive snapshots, foreshadow categories, relation index, or rebuild INDEX.md"
    }
    fn input_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "operation": {
                    "type": "string",
                    "enum": ["character_snapshot", "foreshadow_categories", "relation_index", "rebuild_index"]
                },
                "character_path": {"type": "string", "description": "Relative path for characterSnapshot, e.g. knowledge/characters/林若烟.md"}
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
        run_knowledge_derive_op(&store, &operation, &input)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{PermissionMode, ToolContext};
    use tempfile::TempDir;

    #[tokio::test(flavor = "current_thread")]
    async fn rebuild_index_happy_path() {
        let tmp = TempDir::new().unwrap();
        std::fs::create_dir_all(tmp.path().join("knowledge/characters")).unwrap();
        let ctx = ToolContext {
            permission_mode: PermissionMode::Auto,
            project_root: tmp.path().to_path_buf(),
            ..ToolContext::new(tmp.path().to_path_buf())
        };
        let out = KnowledgeDeriveTool
            .call(json!({"operation": "rebuild_index"}), &ctx)
            .await
            .unwrap();
        assert!(out.content.contains("知识库索引"));
        assert!(!out.is_error);
    }

    #[test]
    fn unknown_operation_errors() {
        let tmp = TempDir::new().unwrap();
        let store = KnowledgeStore::new(tmp.path());
        let err = run_knowledge_derive_op(&store, "nope", &json!({})).unwrap_err();
        assert!(err.to_string().contains("unknown operation"));
    }

    #[test]
    fn character_snapshot_writes_derived_content() {
        const CARD: &str = r#"---
name: 林若烟
aliases: []
category: human
firstAppearance: Ch1
lastUpdate: Ch3
status: alive
povCharacter: true
---

## 身份演变日志
| 章节 | 身份 | 触发事件 |
|------|------|---------|
| Ch3 | 内门弟子 | 考核通过 |

## 出场记录日志
| 章节 | 关键事件 | 伏笔 | 情绪 |
|------|---------|------|------|
| Ch3 | 修炼 | — | 专注 |
"#;
        let tmp = TempDir::new().unwrap();
        std::fs::create_dir_all(tmp.path().join("knowledge/characters")).unwrap();
        std::fs::write(tmp.path().join("knowledge/characters/林若烟.md"), CARD).unwrap();
        let store = KnowledgeStore::new(tmp.path());
        let out = run_knowledge_derive_op(
            &store,
            "character_snapshot",
            &json!({"character_path": "knowledge/characters/林若烟.md"}),
        )
        .unwrap();
        assert!(out.content.contains("Derived snapshot"));
        let body = store.read_file("knowledge/characters/林若烟.md").unwrap();
        assert!(body.contains("当前状态快照"));
    }

    #[test]
    fn relation_index_writes_file() {
        let tmp = TempDir::new().unwrap();
        std::fs::create_dir_all(tmp.path().join("knowledge/characters")).unwrap();
        let store = KnowledgeStore::new(tmp.path());
        let out = derive_relation_index_op(&store).unwrap();
        assert!(out.content.contains("_关系与称呼索引"));
        assert!(tmp
            .path()
            .join("knowledge/characters/_关系与称呼索引.md")
            .exists());
    }

    #[test]
    fn foreshadow_categories_on_empty_store() {
        let tmp = TempDir::new().unwrap();
        std::fs::create_dir_all(tmp.path().join("knowledge/plot")).unwrap();
        let store = KnowledgeStore::new(tmp.path());
        let out = run_knowledge_derive_op(&store, "foreshadow_categories", &json!({})).unwrap();
        assert!(!out.is_error);
    }
}
