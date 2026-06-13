use crate::{ToolContext, ToolError};
use serde_json::Value;
use std::sync::Arc;

mod streaming;

pub use streaming::StreamingToolExecutor;

#[derive(Clone)]
pub struct ToolCallSpec {
    pub id: String,
    pub name: String,
    pub input: Value,
}

#[derive(Clone)]
pub struct ToolExecutor {
    registry: Arc<crate::ToolRegistry>,
}

impl ToolExecutor {
    pub fn new(registry: Arc<crate::ToolRegistry>) -> Self {
        Self { registry }
    }

    pub async fn execute_one(
        &self,
        call: &ToolCallSpec,
        ctx: &ToolContext,
    ) -> Result<crate::ToolOutput, ToolError> {
        self.execute_one_with_approval(call, ctx, false).await
    }

    /// Run a tool after the user explicitly approved a pending `Ask` in the UI.
    pub async fn execute_one_user_approved(
        &self,
        call: &ToolCallSpec,
        ctx: &ToolContext,
    ) -> Result<crate::ToolOutput, ToolError> {
        self.execute_one_with_approval(call, ctx, true).await
    }

    async fn execute_one_with_approval(
        &self,
        call: &ToolCallSpec,
        ctx: &ToolContext,
        user_approved: bool,
    ) -> Result<crate::ToolOutput, ToolError> {
        let tool = self.registry.resolve(&call.name)?;
        tool.validate_input(&call.input)?;
        let mut ctx = ctx.clone();
        ctx.current_tool_call_id = Some(call.id.clone());
        if let Some(reason) = crate::subagent_mutator_gate(&call.name, &ctx) {
            tracing::debug!(tool = %call.name, "subagent_mutator_denied");
            return Err(ToolError::PermissionDenied(reason));
        }
        match tool.check_permissions(&call.input, &ctx) {
            crate::PermissionResult::Allow => {}
            crate::PermissionResult::Deny { reason } => {
                return Err(ToolError::PermissionDenied(reason));
            }
            crate::PermissionResult::Ask { .. } if user_approved => {}
            crate::PermissionResult::Ask { .. } => {
                return Err(ToolError::PermissionDenied("requires user approval".into()));
            }
        }
        tool.call(call.input.clone(), &ctx).await
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::abort::{AbortSignal, InterruptBehavior};
    use crate::{default_registry, PermissionMode};
    use std::sync::Arc;
    use tempfile::TempDir;
    use tokio::sync::watch;

    #[tokio::test]
    async fn partition_read_only_concurrent() {
        let reg = Arc::new(default_registry());
        let ex = ToolExecutor::new(reg);
        let r = ex.registry.get("Read").unwrap();
        assert!(r.is_concurrency_safe());
        let e = ex.registry.get("Edit").unwrap();
        assert!(!e.is_concurrency_safe());
    }

    #[tokio::test]
    async fn read_tool_is_cancel_on_interrupt() {
        let reg = Arc::new(default_registry());
        assert_eq!(
            reg.get("Read").unwrap().interrupt_behavior(),
            InterruptBehavior::Cancel
        );
        assert_eq!(
            reg.get("Write").unwrap().interrupt_behavior(),
            InterruptBehavior::Block
        );
    }

    #[tokio::test]
    async fn abort_generates_synthetic_results() {
        let tmp = TempDir::new().unwrap();
        let reg = Arc::new(default_registry());
        let ctx = ToolContext {
            permission_mode: PermissionMode::Auto,
            project_root: tmp.path().to_path_buf(),
            ..ToolContext::new(tmp.path().to_path_buf())
        };
        let (tx, rx) = watch::channel(AbortSignal::None);
        let mut ex = StreamingToolExecutor::new(reg, ctx, 4, rx);
        ex.add_tool(ToolCallSpec {
            id: "t1".into(),
            name: "Read".into(),
            input: serde_json::json!({"file_path": "missing.txt"}),
        });
        tx.send(AbortSignal::UserCancel).unwrap();
        let results = ex.get_remaining_results().await;
        assert!(!results.is_empty());
    }

    #[tokio::test]
    async fn peek_completed_does_not_drain_for_final_collect() {
        let tmp = TempDir::new().unwrap();
        let path = tmp.path().join("peek.txt");
        std::fs::write(&path, "hello").unwrap();
        let reg = Arc::new(default_registry());
        let ctx = ToolContext {
            permission_mode: PermissionMode::Auto,
            project_root: tmp.path().to_path_buf(),
            ..ToolContext::new(tmp.path().to_path_buf())
        };
        let (tx, rx) = watch::channel(AbortSignal::None);
        let mut ex = StreamingToolExecutor::new(reg, ctx, 4, rx);
        ex.add_tool(ToolCallSpec {
            id: "r1".into(),
            name: "Read".into(),
            input: serde_json::json!({"file_path": "peek.txt"}),
        });

        for _ in 0..100 {
            if !ex.peek_completed_results().is_empty() {
                break;
            }
            tokio::time::sleep(std::time::Duration::from_millis(5)).await;
        }
        assert!(
            !ex.peek_completed_results().is_empty(),
            "tool should finish"
        );
        assert!(ex
            .peek_completed_results()
            .iter()
            .any(|(id, r)| id == "r1" && r.is_ok()));
        let final_results = ex.get_remaining_results().await;
        assert!(
            final_results.iter().any(|(id, r)| id == "r1" && r.is_ok()),
            "polled UI results must still be collected for the LLM turn"
        );
        let _ = tx;
    }

    #[tokio::test]
    async fn concurrent_tools_do_not_trigger_circuit_breaker() {
        let tmp = TempDir::new().unwrap();
        for name in ["a.txt", "b.txt", "c.txt"] {
            std::fs::write(tmp.path().join(name), "x").unwrap();
        }
        let reg = Arc::new(default_registry());
        let ctx = ToolContext {
            permission_mode: PermissionMode::Auto,
            project_root: tmp.path().to_path_buf(),
            ..ToolContext::new(tmp.path().to_path_buf())
        };
        let (tx, rx) = watch::channel(AbortSignal::None);
        let mut ex = StreamingToolExecutor::new(reg, ctx, 4, rx);
        for (id, file) in [("r1", "a.txt"), ("r2", "b.txt"), ("r3", "c.txt")] {
            ex.add_tool(ToolCallSpec {
                id: id.into(),
                name: "Read".into(),
                input: serde_json::json!({"file_path": file}),
            });
        }
        let results = ex.get_remaining_results().await;
        assert_eq!(
            results.len(),
            3,
            "expected one result per tool, got {:?}",
            results
        );
        for (id, r) in &results {
            assert!(r.is_ok(), "tool {id} should succeed, got {r:?}");
            assert!(
                !format!("{r:?}").contains("circuit breaker"),
                "unexpected circuit breaker for {id}"
            );
        }
        let _ = tx;
    }

    #[tokio::test]
    async fn unknown_tool_errors() {
        let tmp = TempDir::new().unwrap();
        let reg = Arc::new(default_registry());
        let ex = ToolExecutor::new(reg);
        let ctx = ToolContext {
            permission_mode: PermissionMode::Auto,
            project_root: tmp.path().to_path_buf(),
            ..ToolContext::new(tmp.path().to_path_buf())
        };
        let err = ex
            .execute_one(
                &ToolCallSpec {
                    id: "1".into(),
                    name: "NoSuchTool".into(),
                    input: serde_json::json!({}),
                },
                &ctx,
            )
            .await
            .unwrap_err();
        assert!(matches!(err, ToolError::UnknownTool(_)));
    }

    #[tokio::test]
    async fn executor_denies_write_on_subagent_context() {
        let tmp = TempDir::new().unwrap();
        let target = tmp.path().join("blocked.md");
        let reg = Arc::new(default_registry());
        let ex = ToolExecutor::new(reg);
        let ctx = ToolContext {
            permission_mode: PermissionMode::Auto,
            allow_fork: false,
            subagent_queue: None,
            project_root: tmp.path().to_path_buf(),
            ..ToolContext::new(tmp.path().to_path_buf())
        };
        let err = ex
            .execute_one(
                &ToolCallSpec {
                    id: "w1".into(),
                    name: "Write".into(),
                    input: serde_json::json!({
                        "file_path": "blocked.md",
                        "content": "nope"
                    }),
                },
                &ctx,
            )
            .await
            .unwrap_err();
        assert!(matches!(err, ToolError::PermissionDenied(_)));
        assert!(!target.exists());
    }
}
