#[cfg(test)]
mod bash_tests {
    use crate::{default_registry, PermissionMode, ToolContext, ToolExecutor};
    use serde_json::json;
    use std::sync::Arc;
    use tempfile::TempDir;

    #[tokio::test(flavor = "current_thread")]
    async fn bash_lists_directory() {
        let tmp = TempDir::new().unwrap();
        std::fs::write(tmp.path().join("hello.txt"), "x").unwrap();
        let reg = Arc::new(default_registry(tmp.path().to_path_buf()));
        let ex = ToolExecutor::new(reg);
        let ctx = ToolContext {
            permission_mode: PermissionMode::Auto,
            project_root: tmp.path().to_path_buf(),
            ..ToolContext::new(tmp.path().to_path_buf())
        };
        let out = ex
            .execute_one(
                &crate::ToolCallSpec {
                    id: "1".into(),
                    name: "Bash".into(),
                    input: json!({"command": if cfg!(windows) { "dir" } else { "ls" }}),
                },
                &ctx,
            )
            .await
            .unwrap();
        assert!(!out.content.is_empty());
    }

    #[tokio::test(flavor = "current_thread")]
    async fn bash_blocks_rm_rf() {
        let tmp = TempDir::new().unwrap();
        let reg = Arc::new(default_registry(tmp.path().to_path_buf()));
        let ex = ToolExecutor::new(reg);
        let ctx = ToolContext {
            permission_mode: PermissionMode::Auto,
            project_root: tmp.path().to_path_buf(),
            ..ToolContext::new(tmp.path().to_path_buf())
        };
        let err = ex
            .execute_one(
                &crate::ToolCallSpec {
                    id: "1".into(),
                    name: "Bash".into(),
                    input: json!({"command": "rm -rf /"}),
                },
                &ctx,
            )
            .await
            .unwrap_err();
        assert!(matches!(err, crate::ToolError::PermissionDenied(_)));
    }
}

#[cfg(test)]
mod novel_tools_tests {
    use crate::{default_registry, PermissionMode, ToolContext, ToolExecutor};
    use serde_json::json;
    use std::sync::Arc;
    use tempfile::TempDir;

    #[tokio::test(flavor = "current_thread")]
    async fn default_registry_has_all_novel_tools() {
        let tmp = TempDir::new().unwrap();
        let reg = default_registry(tmp.path().to_path_buf());
        let names = [
            "Read",
            "Write",
            "Edit",
            "Grep",
            "Glob",
            "Bash",
            "CharacterSearch",
            "PlotGraph",
            "ConsistencyCheck",
            "Tail",
            "WebSearch",
            "PlotGrid",
            "ForeshadowTracker",
            "Stats",
            "Corkboard",
            "CharacterRotate",
        ];
        for name in names {
            assert!(reg.get(name).is_some(), "missing tool {name}");
        }
    }

    #[tokio::test(flavor = "current_thread")]
    async fn foreshadow_tracker_via_executor() {
        let tmp = TempDir::new().unwrap();
        std::fs::create_dir_all(tmp.path().join("knowledge/plot")).unwrap();
        std::fs::write(
            tmp.path().join("knowledge/plot/伏笔追踪.md"),
            "| 章节 | 伏笔ID | 操作 | 内容描述 | 状态 | 预计回收章 | 关联人物 |\n\
             |------|--------|------|---------|------|-----------|----------|\n\
             | Ch5 | F01 | 埋设 | 伤疤 | 待回收 | Ch10 | 陈默 |\n",
        )
        .unwrap();
        let reg = Arc::new(default_registry(tmp.path().to_path_buf()));
        let ex = ToolExecutor::new(reg);
        let ctx = ToolContext {
            permission_mode: PermissionMode::Auto,
            project_root: tmp.path().to_path_buf(),
            ..ToolContext::new(tmp.path().to_path_buf())
        };
        let out = ex
            .execute_one(
                &crate::ToolCallSpec {
                    id: "1".into(),
                    name: "ForeshadowTracker".into(),
                    input: json!({"current_chapter": "Ch8", "warningThreshold": 5}),
                },
                &ctx,
            )
            .await
            .unwrap();
        assert!(out.content.contains("F01"));
    }
}

#[cfg(test)]
mod permission_tests {
    use crate::{default_registry, PermissionMode, ToolContext, ToolExecutor};
    use serde_json::json;
    use std::sync::Arc;
    use tempfile::TempDir;

    #[test]
    fn plan_mode_allows_write_under_plan_only() {
        use crate::PermissionResult;
        let tmp = TempDir::new().unwrap();
        let reg = default_registry(tmp.path().to_path_buf());
        let write = reg.get("Write").expect("Write");
        let ctx = ToolContext {
            permission_mode: PermissionMode::Plan,
            project_root: tmp.path().to_path_buf(),
            ..ToolContext::new(tmp.path().to_path_buf())
        };
        let ok = write.check_permissions(
            &json!({"file_path": "plan/outline.md", "content": "x"}),
            &ctx,
        );
        assert!(matches!(ok, PermissionResult::Allow));
        let bad = write.check_permissions(
            &json!({"file_path": "knowledge/plot/大纲.md", "content": "x"}),
            &ctx,
        );
        assert!(matches!(bad, PermissionResult::Deny { .. }));
    }

    #[test]
    fn todo_write_allowed_in_normal_mode() {
        use crate::PermissionResult;
        let tmp = TempDir::new().unwrap();
        let reg = default_registry(tmp.path().to_path_buf());
        let tool = reg.get("TodoWrite").expect("TodoWrite registered");
        let ctx = ToolContext {
            permission_mode: PermissionMode::Normal,
            project_root: tmp.path().to_path_buf(),
            ..ToolContext::new(tmp.path().to_path_buf())
        };
        let perm = tool.check_permissions(&json!({"todos": []}), &ctx);
        assert!(matches!(perm, PermissionResult::Allow));
    }

    #[tokio::test(flavor = "current_thread")]
    async fn user_approved_write_bypasses_normal_mode_ask() {
        let tmp = TempDir::new().unwrap();
        let reg = Arc::new(default_registry(tmp.path().to_path_buf()));
        let ex = ToolExecutor::new(reg);
        let ctx = ToolContext {
            permission_mode: PermissionMode::Normal,
            project_root: tmp.path().to_path_buf(),
            ..ToolContext::new(tmp.path().to_path_buf())
        };
        let spec = crate::ToolCallSpec {
            id: "w1".into(),
            name: "Write".into(),
            input: json!({"file_path": "approved.md", "content": "ok"}),
        };
        assert!(matches!(
            ex.execute_one(&spec, &ctx).await,
            Err(crate::ToolError::PermissionDenied(_))
        ));
        ex.execute_one_user_approved(&spec, &ctx).await.unwrap();
        assert!(tmp.path().join("approved.md").exists());
    }

    #[tokio::test(flavor = "current_thread")]
    async fn glob_accepts_search_root() {
        use crate::Tool;
        let tmp = TempDir::new().unwrap();
        std::fs::write(tmp.path().join("hello.txt"), "x").unwrap();
        let ctx = ToolContext {
            permission_mode: PermissionMode::Normal,
            project_root: tmp.path().to_path_buf(),
            ..ToolContext::new(tmp.path().to_path_buf())
        };
        let out = crate::builtin::GlobTool
            .call(
                json!({"glob_pattern": "*hello*", "search_root": "."}),
                &ctx,
            )
            .await
            .unwrap();
        assert!(out.content.contains("hello.txt"));
    }

    #[tokio::test(flavor = "current_thread")]
    async fn write_accepts_file_path_alias() {
        let tmp = TempDir::new().unwrap();
        let reg = Arc::new(default_registry(tmp.path().to_path_buf()));
        let ex = ToolExecutor::new(reg);
        let ctx = ToolContext {
            permission_mode: PermissionMode::Auto,
            project_root: tmp.path().to_path_buf(),
            ..ToolContext::new(tmp.path().to_path_buf())
        };
        ex.execute_one(
            &crate::ToolCallSpec {
                id: "w1".into(),
                name: "Write".into(),
                input: json!({"file_path": "ch21.md", "content": "# Ch21"}),
            },
            &ctx,
        )
        .await
        .unwrap();
        assert!(tmp.path().join("ch21.md").exists());
    }
}
