use crate::abort::{AbortSignal, AbortWatch, InterruptBehavior, REJECT_MESSAGE};
use crate::{ToolContext, ToolError};
use serde_json::Value;
use std::collections::{HashSet, VecDeque};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use tokio::sync::{watch, Semaphore};

pub const DEFAULT_MAX_CONCURRENT_TOOLS: usize = 10;

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
        match tool.check_permissions(&call.input, ctx) {
            crate::PermissionResult::Allow => {}
            crate::PermissionResult::Deny { reason } => {
                return Err(ToolError::PermissionDenied(reason));
            }
            crate::PermissionResult::Ask { .. } if user_approved => {}
            crate::PermissionResult::Ask { .. } => {
                return Err(ToolError::PermissionDenied("requires user approval".into()));
            }
        }
        tool.call(call.input.clone(), ctx).await
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum TrackedStatus {
    Queued,
    Executing,
}

enum ToolAbortReason {
    UserInterrupted,
    SiblingError,
    StreamingFallback,
}

struct TrackedTool {
    call: ToolCallSpec,
    status: TrackedStatus,
}

pub struct StreamingToolExecutor {
    queue: VecDeque<TrackedTool>,
    completed: Arc<Mutex<Vec<(String, Result<crate::ToolOutput, ToolError>)>>>,
    concurrency_sem: Arc<Semaphore>,
    serial_lock: Arc<tokio::sync::Mutex<()>>,
    registry: Arc<crate::ToolRegistry>,
    ctx: ToolContext,
    abort: AbortWatch,
    sibling_abort: watch::Sender<AbortSignal>,
    sibling_rx: watch::Receiver<AbortSignal>,
    join_handles: Vec<tokio::task::JoinHandle<()>>,
    discarded: bool,
    has_errored: Arc<AtomicBool>,
    errored_description: Arc<Mutex<String>>,
}

impl StreamingToolExecutor {
    pub fn new(
        registry: Arc<crate::ToolRegistry>,
        ctx: ToolContext,
        max_concurrency: usize,
        abort: AbortWatch,
    ) -> Self {
        let max = max_concurrency.max(1);
        let (sibling_abort, sibling_rx) = watch::channel(AbortSignal::None);
        Self {
            queue: VecDeque::new(),
            completed: Arc::new(Mutex::new(Vec::new())),
            concurrency_sem: Arc::new(Semaphore::new(max)),
            serial_lock: Arc::new(tokio::sync::Mutex::new(())),
            registry,
            ctx,
            abort,
            sibling_abort,
            sibling_rx,
            join_handles: Vec::new(),
            discarded: false,
            has_errored: Arc::new(AtomicBool::new(false)),
            errored_description: Arc::new(Mutex::new(String::new())),
        }
    }

    pub fn discard(&mut self) {
        self.discarded = true;
    }

    pub fn add_tool(&mut self, spec: ToolCallSpec) {
        self.queue.push_back(TrackedTool {
            call: spec,
            status: TrackedStatus::Queued,
        });
        self.try_execute_queued();
    }

    fn current_abort(&self) -> AbortSignal {
        let main = *self.abort.borrow();
        let sibling = *self.sibling_rx.borrow();
        if sibling.is_aborted() {
            sibling
        } else {
            main
        }
    }

    fn get_tool_interrupt_behavior(&self, name: &str) -> InterruptBehavior {
        self.registry
            .get(name)
            .map(|t| t.interrupt_behavior())
            .unwrap_or(InterruptBehavior::Block)
    }

    fn get_abort_reason(&self, tool: &TrackedTool) -> Option<ToolAbortReason> {
        if self.discarded {
            return Some(ToolAbortReason::StreamingFallback);
        }
        if self.has_errored.load(Ordering::SeqCst) {
            return Some(ToolAbortReason::SiblingError);
        }
        let signal = self.current_abort();
        if !signal.is_aborted() {
            return None;
        }
        match signal {
            AbortSignal::SubmitInterrupt => {
                if self.get_tool_interrupt_behavior(&tool.call.name) == InterruptBehavior::Cancel {
                    Some(ToolAbortReason::UserInterrupted)
                } else {
                    None
                }
            }
            AbortSignal::UserCancel
            | AbortSignal::SiblingError
            | AbortSignal::StreamingFallback => Some(ToolAbortReason::UserInterrupted),
            AbortSignal::None => None,
        }
    }

    fn synthetic_error(tool_id: &str, reason: ToolAbortReason, desc: &str) -> (String, ToolError) {
        let content = match reason {
            ToolAbortReason::UserInterrupted => REJECT_MESSAGE.to_string(),
            ToolAbortReason::StreamingFallback => {
                "Error: Streaming fallback - tool execution discarded".into()
            }
            ToolAbortReason::SiblingError => {
                if desc.is_empty() {
                    "Cancelled: parallel tool call errored".into()
                } else {
                    format!("Cancelled: parallel tool call {desc} errored")
                }
            }
        };
        (tool_id.to_string(), ToolError::Internal(content))
    }

    pub fn has_interruptible_tool_in_progress(&self) -> bool {
        let executing: Vec<_> = self
            .queue
            .iter()
            .filter(|t| t.status == TrackedStatus::Executing)
            .collect();
        !executing.is_empty()
            && executing.iter().all(|t| {
                self.get_tool_interrupt_behavior(&t.call.name) == InterruptBehavior::Cancel
            })
    }

    fn try_execute_queued(&mut self) {
        if self.current_abort().is_aborted() {
            return;
        }
        let mut idx = 0;
        while idx < self.queue.len() {
            if self.queue[idx].status != TrackedStatus::Queued {
                idx += 1;
                continue;
            }
            if let Some(reason) = self.get_abort_reason(&self.queue[idx]) {
                let tracked = self.queue.remove(idx).expect("queue idx invariant");
                let (id, err) = Self::synthetic_error(&tracked.call.id, reason, "");
                self.completed
                    .lock()
                    .unwrap_or_else(|e| e.into_inner())
                    .push((id, Err(err)));
                continue;
            }

            let is_concurrent = self
                .registry
                .get(&self.queue[idx].call.name)
                .map(|t| t.is_concurrency_safe())
                .unwrap_or(false);

            if is_concurrent {
                self.queue[idx].status = TrackedStatus::Executing;
                let sem = Arc::clone(&self.concurrency_sem);
                let spec = self.queue[idx].call.clone();
                let registry = Arc::clone(&self.registry);
                let ctx = self.ctx.clone();
                let completed = Arc::clone(&self.completed);
                let has_errored = Arc::clone(&self.has_errored);
                let errored_description = Arc::clone(&self.errored_description);
                let sibling_abort = self.sibling_abort.clone();
                let handle = tokio::spawn(async move {
                    let _permit = sem
                        .acquire_owned()
                        .await
                        .expect("semaphore never closed");
                    let executor = ToolExecutor {
                        registry: registry.clone(),
                    };
                    let id = spec.id.clone();
                    let r = executor.execute_one(&spec, &ctx).await;
                    if spec.name == "Bash" && r.is_err() {
                        has_errored.store(true, Ordering::SeqCst);
                        if let Ok(mut d) = errored_description.lock() {
                            *d = format!(
                                "Bash({})",
                                spec.input
                                    .get("command")
                                    .and_then(|v| v.as_str())
                                    .unwrap_or("")
                            );
                        }
                        let _ = sibling_abort.send(AbortSignal::SiblingError);
                    }
                    completed
                        .lock()
                        .unwrap_or_else(|e| e.into_inner())
                        .push((id, r));
                });
                self.join_handles.push(handle);
                idx += 1;
            } else {
                let tracked = self.queue.remove(idx).expect("queue idx invariant");
                let spec = tracked.call;
                let registry = Arc::clone(&self.registry);
                let ctx = self.ctx.clone();
                let completed = Arc::clone(&self.completed);
                let lock = Arc::clone(&self.serial_lock);
                let handle = tokio::spawn(async move {
                    let _guard = lock.lock().await;
                    let executor = ToolExecutor { registry };
                    let id = spec.id.clone();
                    let r = executor.execute_one(&spec, &ctx).await;
                    completed
                        .lock()
                        .unwrap_or_else(|e| e.into_inner())
                        .push((id, r));
                });
                self.join_handles.push(handle);
            }
        }
    }

    /// Snapshot finished tool results without removing them (for UI polling during SSE).
    pub fn peek_completed_results(&self) -> Vec<(String, Result<crate::ToolOutput, ToolError>)> {
        self.completed
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .iter()
            .map(|(id, r)| {
                (
                    id.clone(),
                    match r {
                        Ok(o) => Ok(o.clone()),
                        Err(e) => Err(ToolError::Internal(e.to_string())),
                    },
                )
            })
            .collect()
    }

    /// Take all finished tool results (end of stream / turn). Drains the internal buffer.
    pub fn get_completed_results(&mut self) -> Vec<(String, Result<crate::ToolOutput, ToolError>)> {
        let mut completed = self.completed.lock().unwrap_or_else(|e| e.into_inner());
        std::mem::take(&mut *completed)
    }

    /// Concurrent tools stay in `queue` as `Executing` until their task finishes; remove them
    /// once a result is recorded so `get_remaining_results` does not treat them as stalled.
    fn prune_finished_executing(&mut self) {
        let done: HashSet<String> = self
            .completed
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .iter()
            .map(|(id, _)| id.clone())
            .collect();
        self.queue.retain(|t| {
            !(t.status == TrackedStatus::Executing && done.contains(&t.call.id))
        });
    }

    fn pending_count(&self, in_flight_handles: usize) -> usize {
        self.queue
            .iter()
            .filter(|t| t.status == TrackedStatus::Queued)
            .count()
            + in_flight_handles
    }

    fn flush_aborted_tools(&mut self) {
        let desc = self
            .errored_description
            .lock()
            .map(|g| g.clone())
            .unwrap_or_default();
        let drained: Vec<_> = self.queue.drain(..).collect();
        let mut completed = self.completed.lock().unwrap_or_else(|e| e.into_inner());
        for tracked in drained {
            if let Some(reason) = self.get_abort_reason(&tracked) {
                let (id, err) = Self::synthetic_error(&tracked.call.id, reason, &desc);
                completed.push((id, Err(err)));
            }
        }
    }

    pub async fn get_remaining_results(
        &mut self,
    ) -> Vec<(String, Result<crate::ToolOutput, ToolError>)> {
        let mut stall_count = 0u32;
        let mut prev_pending = usize::MAX;
        loop {
            if self.current_abort().is_aborted() {
                for h in self.join_handles.drain(..) {
                    h.abort();
                }
                self.flush_aborted_tools();
                break;
            }

            self.try_execute_queued();
            let handles = std::mem::take(&mut self.join_handles);
            let handle_count = handles.len();

            if self.current_abort().is_aborted() {
                for h in handles {
                    h.abort();
                }
                self.flush_aborted_tools();
                break;
            }

            for h in handles {
                let _ = h.await;
            }
            self.prune_finished_executing();

            let pending = self.pending_count(0);
            if pending == 0 {
                break;
            }
            if pending >= prev_pending {
                stall_count += 1;
                if stall_count > 10 {
                    tracing::warn!(
                        queued = self
                            .queue
                            .iter()
                            .filter(|t| t.status == TrackedStatus::Queued)
                            .count(),
                        executing = self
                            .queue
                            .iter()
                            .filter(|t| t.status == TrackedStatus::Executing)
                            .count(),
                        in_flight = handle_count,
                        "streaming_tool_executor_circuit_breaker"
                    );
                    let mut completed = self.completed.lock().unwrap_or_else(|e| e.into_inner());
                    while let Some(call) = self.queue.pop_front() {
                        let id = call.call.id.clone();
                        if completed.iter().any(|(cid, r)| cid == &id && r.is_ok()) {
                            continue;
                        }
                        completed.push((
                            id,
                            Err(ToolError::Internal(
                                "tool execution stalled — aborted by circuit breaker".into(),
                            )),
                        ));
                    }
                    return std::mem::take(&mut *completed);
                }
            } else {
                stall_count = 0;
            }
            prev_pending = pending;
        }
        self.get_completed_results()
    }

    pub fn has_pending(&self) -> bool {
        !self.queue.is_empty() || !self.join_handles.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{default_registry, PermissionMode};
    use std::sync::Arc;
    use tempfile::TempDir;

    #[tokio::test]
    async fn partition_read_only_concurrent() {
        let tmp = TempDir::new().unwrap();
        let reg = Arc::new(default_registry(tmp.path().to_path_buf()));
        let ex = ToolExecutor::new(reg);
        let r = ex.registry.get("Read").unwrap();
        assert!(r.is_concurrency_safe());
        let e = ex.registry.get("Edit").unwrap();
        assert!(!e.is_concurrency_safe());
    }

    #[tokio::test]
    async fn read_tool_is_cancel_on_interrupt() {
        let tmp = TempDir::new().unwrap();
        let reg = Arc::new(default_registry(tmp.path().to_path_buf()));
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
        let reg = Arc::new(default_registry(tmp.path().to_path_buf()));
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
        let reg = Arc::new(default_registry(tmp.path().to_path_buf()));
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
        assert!(!ex.peek_completed_results().is_empty(), "tool should finish");
        // UI poll uses peek — buffer must still be intact for turn finalization.
        assert!(
            ex.peek_completed_results()
                .iter()
                .any(|(id, r)| id == "r1" && r.is_ok())
        );
        let final_results = ex.get_remaining_results().await;
        assert!(
            final_results
                .iter()
                .any(|(id, r)| id == "r1" && r.is_ok()),
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
        let reg = Arc::new(default_registry(tmp.path().to_path_buf()));
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
        assert_eq!(results.len(), 3, "expected one result per tool, got {:?}", results);
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
        let reg = Arc::new(default_registry(tmp.path().to_path_buf()));
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
}
