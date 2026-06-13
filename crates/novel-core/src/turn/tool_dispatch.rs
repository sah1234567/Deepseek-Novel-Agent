use crate::message::parse_tool_call_input;
use crate::session_todos::maybe_emit_session_todos_after_tool;
use crate::{AgentEngine, Event};
use novel_deepseek::LlmToolCall;
use novel_state::Database;
use novel_tools::{
    format_tool_result_for_llm, FormattedToolResult, PermissionResult, StreamingToolExecutor,
    ToolCallSpec, ToolContext, ToolError, ToolOutput, ToolRegistry, ToolResultSpec,
};
use std::collections::{HashMap, HashSet};
use std::sync::Arc;
use tokio::sync::mpsc;

pub(crate) struct StreamingToolDispatch {
    executor: Option<StreamingToolExecutor>,
    pub(crate) handled_ids: HashSet<String>,
    pub(crate) executed_specs: Vec<ToolCallSpec>,
    pub(crate) pending_specs: HashMap<String, ToolCallSpec>,
    pub(crate) denied_specs: HashMap<String, (ToolCallSpec, String)>,
    pub(crate) ui_result_emitted: HashSet<String>,
}

impl StreamingToolDispatch {
    pub(crate) fn new(
        registry: Arc<ToolRegistry>,
        ctx: ToolContext,
        max_concurrent: usize,
        abort: novel_tools::AbortWatch,
    ) -> Self {
        Self {
            executor: Some(StreamingToolExecutor::new(
                registry,
                ctx,
                max_concurrent,
                abort,
            )),
            handled_ids: HashSet::new(),
            executed_specs: Vec::new(),
            pending_specs: HashMap::new(),
            denied_specs: HashMap::new(),
            ui_result_emitted: HashSet::new(),
        }
    }

    pub(crate) fn executor_mut(&mut self) -> Option<&mut StreamingToolExecutor> {
        self.executor.as_mut()
    }

    pub(crate) fn take_executor(&mut self) -> Option<StreamingToolExecutor> {
        self.executor.take()
    }

    /// Move tools that were streamed and queued for approval into the engine's `pending_tools`.
    pub(crate) fn drain_pending_specs(&mut self, engine: &mut AgentEngine) {
        for (_, spec) in self.pending_specs.drain() {
            tracing::debug!(
                tool_call_id = %spec.id,
                tool_name = %spec.name,
                "flush_pending_tool_approval"
            );
            engine.pending_tools.insert(spec.id.clone(), spec);
        }
    }

    pub(crate) fn handle_ready(
        &mut self,
        registry: &ToolRegistry,
        ctx: &ToolContext,
        event_tx: Option<&mpsc::UnboundedSender<Event>>,
        tc: LlmToolCall,
        finalize: bool,
    ) {
        if self.handled_ids.contains(&tc.id) {
            return;
        }
        let input = parse_tool_call_input(&tc.arguments, &tc.id, &tc.name);
        if let Some(reason) = tool_validation_failure(registry, &tc, &input) {
            if !finalize {
                tracing::debug!(
                    tool_call_id = %tc.id,
                    tool_name = %tc.name,
                    %reason,
                    "tool_input_deferred"
                );
                return;
            }
            tracing::debug!(
                tool_call_id = %tc.id,
                tool_name = %tc.name,
                %reason,
                "tool_input_invalid_at_finalize"
            );
            self.record_denied_tool(&tc.id, &tc.name, input, reason);
            return;
        }
        if registry.get(&tc.name).is_none() {
            if !finalize {
                return;
            }
            self.record_denied_tool(&tc.id, &tc.name, input, "unknown tool".into());
            return;
        }

        let perm = registry
            .get(&tc.name)
            .map(|t| t.check_permissions(&input, ctx))
            .unwrap_or(PermissionResult::Deny {
                reason: "unknown tool".into(),
            });
        let needs_approval = matches!(perm, PermissionResult::Ask { .. });

        if let Some(tx) = event_tx {
            let _ = tx.send(Event::ToolInputComplete {
                tool_call_id: tc.id.clone(),
                name: tc.name.clone(),
                input: input.clone(),
                needs_approval,
            });
        }

        self.handled_ids.insert(tc.id.clone());
        let spec = ToolCallSpec {
            id: tc.id.clone(),
            name: tc.name.clone(),
            input,
        };
        self.apply_permission(spec, perm, event_tx);
    }

    fn record_denied_tool(
        &mut self,
        id: &str,
        name: &str,
        input: serde_json::Value,
        reason: String,
    ) {
        self.handled_ids.insert(id.to_string());
        let spec = ToolCallSpec {
            id: id.to_string(),
            name: name.to_string(),
            input,
        };
        self.denied_specs.insert(spec.id.clone(), (spec, reason));
    }

    fn apply_permission(
        &mut self,
        spec: ToolCallSpec,
        perm: PermissionResult,
        event_tx: Option<&mpsc::UnboundedSender<Event>>,
    ) {
        match perm {
            PermissionResult::Allow => {
                if let Some(tx) = event_tx {
                    let _ = tx.send(Event::ToolCallRequest {
                        tool_call_id: spec.id.clone(),
                        name: spec.name.clone(),
                        input: spec.input.clone(),
                        needs_approval: false,
                    });
                }
                self.executed_specs.push(spec.clone());
                if let Some(executor) = self.executor_mut() {
                    executor.add_tool(spec);
                } else {
                    tracing::warn!(
                        tool_call_id = %spec.id,
                        tool_name = %spec.name,
                        "streaming tool executor missing; tool not queued"
                    );
                }
            }
            PermissionResult::Ask { .. } => {
                tracing::debug!(
                    tool_call_id = %spec.id,
                    tool_name = %spec.name,
                    "tool_permission_ask"
                );
                if let Some(tx) = event_tx {
                    let _ = tx.send(Event::ToolCallRequest {
                        tool_call_id: spec.id.clone(),
                        name: spec.name.clone(),
                        input: spec.input.clone(),
                        needs_approval: true,
                    });
                }
                self.pending_specs.insert(spec.id.clone(), spec);
            }
            PermissionResult::Deny { reason } => {
                tracing::debug!(
                    tool_call_id = %spec.id,
                    tool_name = %spec.name,
                    %reason,
                    "tool_permission_denied"
                );
                self.denied_specs.insert(spec.id.clone(), (spec, reason));
            }
        }
    }
}

fn tool_validation_failure(
    registry: &ToolRegistry,
    tc: &LlmToolCall,
    input: &serde_json::Value,
) -> Option<String> {
    let err = if tc.arguments.trim().is_empty() {
        registry
            .get(&tc.name)
            .map(|t| t.validate_input(input).map_err(|e| e.to_string()))
    } else if input.as_object().is_some_and(|o| o.is_empty()) {
        Some(Err(format!("Invalid tool arguments JSON for {}", tc.name)))
    } else {
        registry
            .get(&tc.name)
            .map(|t| t.validate_input(input).map_err(|e| e.to_string()))
    };
    match err {
        Some(Err(reason)) => Some(reason),
        _ => None,
    }
}

impl StreamingToolDispatch {
    pub(crate) fn poll_ui_results(
        &mut self,
        registry: &ToolRegistry,
        event_tx: Option<&mpsc::UnboundedSender<Event>>,
        todo_refresh: Option<(&str, &Database)>,
    ) {
        // Must not drain `completed` —results are collected once in get_remaining_results
        // for persistence and the next LLM call. Draining here caused UI success + missing
        // tool_result messages (model reports "timeout" / retries InvokeSkill).
        let Some(executor) = self.executor_mut() else {
            return;
        };
        let completed = executor.peek_completed_results();
        let peek_count = completed.len();
        if peek_count > 0 {
            tracing::debug!(peek_count, "poll_ui_results peeked completed tool results");
        }
        for (id, result) in completed {
            if !self.ui_result_emitted.insert(id.clone()) {
                continue;
            }
            let spec = self.executed_specs.iter().find(|s| s.id == id);
            let content = format_tool(registry, spec, result).content;
            if let Some(tx) = event_tx {
                let _ = tx.send(Event::ToolCallResult {
                    tool_call_id: id.clone(),
                    content,
                });
            }
            if let Some((session_id, db)) = todo_refresh {
                if let Some(s) = spec {
                    maybe_emit_session_todos_after_tool(&s.name, session_id, db, event_tx);
                }
            }
        }
    }

    pub(crate) fn discard(&mut self) {
        if let Some(executor) = self.executor.as_mut() {
            executor.discard();
        }
    }
}

pub(crate) fn format_tool(
    registry: &novel_tools::ToolRegistry,
    spec: Option<&ToolCallSpec>,
    result: Result<ToolOutput, ToolError>,
) -> FormattedToolResult {
    let spec_ref = spec
        .map(|s| ToolResultSpec {
            tool_name: &s.name,
            tool_input: Some(&s.input),
        })
        .unwrap_or(ToolResultSpec {
            tool_name: "",
            tool_input: None,
        });
    format_tool_result_for_llm(registry, spec_ref, result)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Event;
    use novel_deepseek::LlmToolCall;
    use novel_tools::default_registry;
    use novel_tools::PermissionMode;
    use std::sync::Arc;
    use tempfile::TempDir;
    use tokio::sync::mpsc;

    #[tokio::test(flavor = "current_thread")]
    async fn poll_ui_results_emits_once_per_tool() {
        let tmp = TempDir::new().unwrap();
        std::fs::write(tmp.path().join("peek.txt"), "hello").unwrap();
        let reg = Arc::new(default_registry());
        let ctx = ToolContext {
            permission_mode: PermissionMode::Auto,
            project_root: tmp.path().to_path_buf(),
            ..ToolContext::new(tmp.path().to_path_buf())
        };
        let (abort_tx, abort_rx) = novel_tools::abort_channel();
        let _ = abort_tx;
        let mut dispatch = StreamingToolDispatch::new(reg.clone(), ctx.clone(), 4, abort_rx);
        let tc = LlmToolCall {
            id: "r1".into(),
            name: "Read".into(),
            arguments: r#"{"file_path":"peek.txt"}"#.into(),
        };
        dispatch.handle_ready(&reg, &ctx, None, tc, true);

        for _ in 0..100 {
            let done = dispatch
                .executor_mut()
                .is_some_and(|e| !e.peek_completed_results().is_empty());
            if done {
                break;
            }
            tokio::time::sleep(std::time::Duration::from_millis(5)).await;
        }

        let (ev_tx, mut ev_rx) = mpsc::unbounded_channel();
        dispatch.poll_ui_results(&reg, Some(&ev_tx), None);
        dispatch.poll_ui_results(&reg, Some(&ev_tx), None);
        let first = ev_rx.try_recv().expect("ToolCallResult event");
        assert!(matches!(first, Event::ToolCallResult { .. }));
        assert!(ev_rx.try_recv().is_err());
    }

    #[tokio::test(flavor = "current_thread")]
    async fn poll_ui_results_emits_session_todos_after_todo_write() {
        let tmp = TempDir::new().unwrap();
        let db = novel_state::Database::open(tmp.path().join("todo_poll.db")).unwrap();
        let sid = db.create_session("/tmp/proj", "deepseek-chat").unwrap();
        let reg = Arc::new(default_registry());
        let queue = Arc::new(std::sync::Mutex::new(Vec::new()));
        let ctx = ToolContext {
            permission_mode: PermissionMode::Auto,
            project_root: tmp.path().to_path_buf(),
            session_id: sid.clone(),
            db: Some(Arc::new(db.clone())),
            subagent_queue: Some(queue),
            ..ToolContext::new(tmp.path().to_path_buf())
        };
        let (abort_tx, abort_rx) = novel_tools::abort_channel();
        let _ = abort_tx;
        let mut dispatch = StreamingToolDispatch::new(reg.clone(), ctx.clone(), 4, abort_rx);
        let tc = LlmToolCall {
            id: "tw1".into(),
            name: "TodoWrite".into(),
            arguments: r#"{"todos":[{"id":"a","content":"write ch1","status":"in_progress"}]}"#
                .into(),
        };
        dispatch.handle_ready(&reg, &ctx, None, tc, true);

        for _ in 0..100 {
            let done = dispatch
                .executor_mut()
                .is_some_and(|e| !e.peek_completed_results().is_empty());
            if done {
                break;
            }
            tokio::time::sleep(std::time::Duration::from_millis(5)).await;
        }

        let (ev_tx, mut ev_rx) = mpsc::unbounded_channel();
        dispatch.poll_ui_results(&reg, Some(&ev_tx), Some((&sid, &db)));
        let result = ev_rx.try_recv().expect("ToolCallResult");
        assert!(matches!(result, Event::ToolCallResult { .. }));
        let todos_evt = ev_rx.try_recv().expect("SessionTodosUpdated");
        match todos_evt {
            Event::SessionTodosUpdated { todos } => {
                assert_eq!(todos.len(), 1);
                assert_eq!(todos[0].content, "write ch1");
            }
            other => panic!("unexpected event: {other:?}"),
        }
    }

    #[test]
    fn handle_ready_queues_ask_permission_tool() {
        let reg = Arc::new(default_registry());
        let ctx = ToolContext {
            permission_mode: PermissionMode::Normal,
            project_root: std::path::PathBuf::from("."),
            ..ToolContext::new(std::path::PathBuf::from("."))
        };
        let (abort_tx, abort_rx) = novel_tools::abort_channel();
        let _ = abort_tx;
        let mut dispatch = StreamingToolDispatch::new(reg.clone(), ctx.clone(), 4, abort_rx);
        dispatch.handle_ready(
            &reg,
            &ctx,
            None,
            LlmToolCall {
                id: "w1".into(),
                name: "Write".into(),
                arguments: r#"{"file_path":"out.txt","content":"x"}"#.into(),
            },
            true,
        );
        assert!(dispatch.pending_specs.contains_key("w1"));
    }

    #[test]
    fn take_executor_returns_none_when_already_taken() {
        let reg = Arc::new(default_registry());
        let ctx = ToolContext {
            permission_mode: PermissionMode::Auto,
            project_root: std::path::PathBuf::from("."),
            ..ToolContext::new(std::path::PathBuf::from("."))
        };
        let (abort_tx, abort_rx) = novel_tools::abort_channel();
        let _ = abort_tx;
        let mut dispatch = StreamingToolDispatch::new(reg, ctx, 4, abort_rx);
        assert!(dispatch.take_executor().is_some());
        assert!(dispatch.take_executor().is_none());
        assert!(dispatch.executor_mut().is_none());
    }

    #[test]
    fn handle_ready_denied_on_unknown_tool_at_finalize() {
        let reg = Arc::new(default_registry());
        let ctx = ToolContext {
            permission_mode: PermissionMode::Auto,
            project_root: std::path::PathBuf::from("."),
            ..ToolContext::new(std::path::PathBuf::from("."))
        };
        let (abort_tx, abort_rx) = novel_tools::abort_channel();
        let _ = abort_tx;
        let mut dispatch = StreamingToolDispatch::new(reg.clone(), ctx.clone(), 4, abort_rx);
        dispatch.handle_ready(
            &reg,
            &ctx,
            None,
            LlmToolCall {
                id: "x1".into(),
                name: "NotARealTool".into(),
                arguments: "{}".into(),
            },
            true,
        );
        assert!(dispatch.denied_specs.contains_key("x1"));
    }
}
