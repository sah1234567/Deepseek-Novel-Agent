use novel_tools::{default_registry, PermissionMode, ToolCallSpec, ToolContext, ToolExecutor};
use std::sync::Arc;
use tempfile::TempDir;

#[tokio::test]
async fn read_write_edit_chain() {
    let tmp = TempDir::new().unwrap();
    let root = tmp.path().to_path_buf();
    std::fs::create_dir_all(root.join("knowledge")).unwrap();
    let reg = Arc::new(default_registry(root.clone()));
    let ex = ToolExecutor::new(reg);
    let ctx = ToolContext {
        permission_mode: PermissionMode::Auto,
        project_root: root.clone(),
        ..ToolContext::new(root.clone())
    };
    ex.execute_one(
        &ToolCallSpec {
            id: "1".into(),
            name: "Write".into(),
            input: serde_json::json!({
                "file_path": "knowledge/draft.md",
                "content": "林若烟入门。\n陈默旁观。"
            }),
        },
        &ctx,
    )
    .await
    .unwrap();
    let out = ex
        .execute_one(
            &ToolCallSpec {
                id: "2".into(),
                name: "Read".into(),
                input: serde_json::json!({"file_path": "knowledge/draft.md"}),
            },
            &ctx,
        )
        .await
        .unwrap();
    assert!(out.content.contains("林若烟"));
    ex.execute_one(
        &ToolCallSpec {
            id: "3".into(),
            name: "Edit".into(),
            input: serde_json::json!({
                "file_path": "knowledge/draft.md",
                "old_string": "陈默旁观。",
                "new_string": "陈默冷淡旁观。"
            }),
        },
        &ctx,
    )
    .await
    .unwrap();
    let out2 = ex
        .execute_one(
            &ToolCallSpec {
                id: "4".into(),
                name: "Grep".into(),
                input: serde_json::json!({
                    "pattern": "冷淡",
                    "search_root": "."
                }),
            },
            &ctx,
        )
        .await
        .unwrap();
    assert!(out2.content.contains("冷淡"));
}

#[tokio::test]
async fn main_session_may_write_chapter_in_sandbox() {
    let tmp = TempDir::new().unwrap();
    let root = tmp.path().to_path_buf();
    std::fs::create_dir_all(root.join("chapters")).unwrap();
    let reg = Arc::new(default_registry(root.clone()));
    let ex = ToolExecutor::new(reg);
    let ctx = ToolContext {
        permission_mode: PermissionMode::Auto,
        project_root: root.clone(),
        ..ToolContext::new(root)
    };
    ex.execute_one(
        &ToolCallSpec {
            id: "1".into(),
            name: "Write".into(),
            input: serde_json::json!({
                "file_path": "chapters/chapter-001.md",
                "content": "正文"
            }),
        },
        &ctx,
    )
    .await
    .unwrap();
}

#[tokio::test]
async fn main_session_may_call_consistency_check() {
    let tmp = TempDir::new().unwrap();
    let root = tmp.path().to_path_buf();
    std::fs::create_dir_all(root.join("chapters")).unwrap();
    std::fs::create_dir_all(root.join("knowledge/plot")).unwrap();
    std::fs::write(
        root.join("chapters/chapter-001.md"),
        "# 第一章\n\n测试正文。",
    )
    .unwrap();
    let reg = Arc::new(default_registry(root.clone()));
    let ex = ToolExecutor::new(reg);
    let ctx = ToolContext {
        permission_mode: PermissionMode::Auto,
        project_root: root,
        ..ToolContext::new(tmp.path().to_path_buf())
    };
    let out = ex
        .execute_one(
            &ToolCallSpec {
                id: "1".into(),
                name: "ConsistencyCheck".into(),
                input: serde_json::json!({
                    "chapter_path": "chapters/chapter-001.md",
                    "aspects": ["viewpoint"]
                }),
            },
            &ctx,
        )
        .await
        .unwrap();
    assert!(!out.content.is_empty());
}

#[tokio::test]
async fn character_search_finds_name() {
    let tmp = TempDir::new().unwrap();
    let root = tmp.path().to_path_buf();
    std::fs::create_dir_all(root.join("knowledge/characters")).unwrap();
    std::fs::write(
        root.join("knowledge/characters/林若烟.md"),
        "## 性格演变日志\n| Ch1 | 天真 |\n",
    )
    .unwrap();
    let reg = Arc::new(default_registry(root.clone()));
    let ex = ToolExecutor::new(reg);
    let ctx = ToolContext {
        permission_mode: PermissionMode::Auto,
        project_root: root,
        ..ToolContext::new(tmp.path().to_path_buf())
    };
    let out = ex
        .execute_one(
            &ToolCallSpec {
                id: "1".into(),
                name: "CharacterSearch".into(),
                input: serde_json::json!({"query": "天真"}),
            },
            &ctx,
        )
        .await
        .unwrap();
    assert!(out.content.contains("林若烟"));
}
