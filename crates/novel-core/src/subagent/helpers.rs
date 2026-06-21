//! Helpers for [`crate::subagent::runner::run_subagent_job`] (tool context, tool batch, stream events).

use crate::agent::merge_tool_always_allow;
use crate::fork_stream_subs::{try_send_fork_overlay_event, ForkStreamSubscriptions};
use crate::message::{parse_tool_call_input, tool_result_message};
use crate::turn::TurnContext;
use crate::turn::{format_tool, StreamingToolDispatch};
use crate::{AgentError, AgentType, ChatMessage, Event};
use novel_deepseek::LlmCompletion;
use novel_deepseek::{LlmToolCall, StreamEvent};
use novel_tools::{PermissionMode, StreamingToolExecutor, ToolCallSpec, ToolContext, ToolRegistry};
use std::sync::{Arc, Mutex};
use tokio::sync::mpsc;

pub(crate) fn fork_child_push(
    db: &novel_state::Database,
    run_id: &str,
    child: &mut Vec<ChatMessage>,
    msg: ChatMessage,
) -> Result<(), AgentError> {
    crate::subagent::fork_transcript::persist_fork_message(db, run_id, &msg)?;
    child.push(msg);
    Ok(())
}

pub(crate) fn subagent_fork_tool_context(shared: &crate::EngineShared) -> ToolContext {
    ToolContext {
        permission_mode: PermissionMode::from_settings_str(&shared.settings.permissions.mode),
        deny_rules: shared.settings.permissions.deny_rules.clone(),
        always_allow: merge_tool_always_allow(&shared.settings.permissions.always_allow),
        project_root: shared.session.project_root.clone(),
        session_id: shared.session.id.clone(),
        db: Some(Arc::new(shared.session.db.clone())),
        permission_mode_override: Some(Arc::clone(&shared.permission_mode_override)),
        // Subagents do not share the main read_file_cache: promote/dirty_paths are main-only
        // (aligns with Claude Code subagent readFileState isolation).
        read_file_cache: None,
        file_op_locks: Some(Arc::clone(&shared.file_op_locks)),
        allow_fork: false,
        subagent_queue: None,
        current_tool_call_id: None,
        skills_dir: Some(shared.agent_skills_dir.clone()),
        global_api_config_path: Some(shared.global_config_path.clone()),
        on_read_cache_path_touched: None,
        memory_fork_mode: false,
    }
}

pub(crate) fn subagent_memory_fork_tool_context(shared: &crate::EngineShared) -> ToolContext {
    ToolContext {
        memory_fork_mode: true,
        permission_mode: PermissionMode::Auto,
        ..subagent_fork_tool_context(shared)
    }
}

pub(crate) fn subagent_tool_context(
    shared: &crate::EngineShared,
    agent_type: AgentType,
) -> ToolContext {
    if agent_type == AgentType::MemoryExtractor {
        subagent_memory_fork_tool_context(shared)
    } else {
        subagent_fork_tool_context(shared)
    }
}

pub(crate) fn forward_subagent_stream_event(
    subs: &ForkStreamSubscriptions,
    tx: &mpsc::UnboundedSender<Event>,
    fork_run_id: &str,
    ev: StreamEvent,
) {
    let event = match ev {
        StreamEvent::ContentBlockDelta { delta, kind, .. } => Event::SubAgentStreamDelta {
            fork_run_id: fork_run_id.to_string(),
            delta,
            kind,
        },
        StreamEvent::ToolUseStarted {
            tool_call_id, name, ..
        } => Event::SubAgentToolUpdate {
            fork_run_id: fork_run_id.to_string(),
            phase: "start".into(),
            tool_call_id,
            tool_name: Some(name),
            input: None,
            content: None,
            needs_approval: None,
            status: None,
            description: None,
        },
        StreamEvent::ToolInputDelta {
            tool_call_id,
            delta,
        } => Event::SubAgentToolUpdate {
            fork_run_id: fork_run_id.to_string(),
            phase: "input_delta".into(),
            tool_call_id,
            tool_name: None,
            input: None,
            content: Some(delta),
            needs_approval: None,
            status: None,
            description: None,
        },
        StreamEvent::MessageStop { .. } | StreamEvent::StreamError { .. } => return,
    };
    try_send_fork_overlay_event(subs, tx, event);
}

pub(crate) async fn execute_subagent_tool_batch(
    registry: &Arc<ToolRegistry>,
    fork_ctx: &ToolContext,
    fork_dispatch: Option<Arc<Mutex<StreamingToolDispatch>>>,
    tool_calls: &[LlmToolCall],
    event_tx: Option<&mpsc::UnboundedSender<Event>>,
    fork_run_id: &str,
    subs: &ForkStreamSubscriptions,
) -> Result<
    Vec<(
        String,
        Result<novel_tools::ToolOutput, novel_tools::ToolError>,
    )>,
    AgentError,
> {
    if let Some(tx) = event_tx {
        for tc in tool_calls {
            let input = parse_tool_call_input(&tc.arguments, &tc.id, &tc.name);
            try_send_fork_overlay_event(
                subs,
                tx,
                Event::SubAgentToolUpdate {
                    fork_run_id: fork_run_id.to_string(),
                    phase: "input_complete".into(),
                    tool_call_id: tc.id.clone(),
                    tool_name: Some(tc.name.clone()),
                    input: Some(input),
                    content: None,
                    needs_approval: None,
                    status: None,
                    description: None,
                },
            );
        }
    }
    if let Some(dispatch_arc) = fork_dispatch {
        let mut executor = {
            let mut dispatch = dispatch_arc
                .lock()
                .map_err(|_| AgentError::Validation("fork tool dispatch lock poisoned".into()))?;
            for tc in tool_calls {
                if !dispatch.handled_ids.contains(&tc.id) {
                    dispatch.handle_ready(registry, fork_ctx, None, tc.clone(), true);
                }
            }
            dispatch.take_executor().ok_or_else(|| {
                AgentError::Validation("fork streaming tool executor already taken".into())
            })?
        };
        Ok(executor.get_remaining_results().await)
    } else {
        let mut executor = StreamingToolExecutor::new(
            Arc::clone(registry),
            fork_ctx.clone(),
            1,
            novel_tools::abort_channel().1,
        );
        for tc in tool_calls {
            let spec = ToolCallSpec {
                id: tc.id.clone(),
                name: tc.name.clone(),
                input: parse_tool_call_input(&tc.arguments, &tc.id, &tc.name),
            };
            if registry.get(&spec.name).is_some() {
                executor.add_tool(spec);
            }
        }
        Ok(executor.get_remaining_results().await)
    }
}

#[allow(clippy::too_many_arguments)]
pub(crate) fn subagent_push_tool_results(
    registry: &novel_tools::ToolRegistry,
    db: &novel_state::Database,
    fork_run_id: &str,
    child_messages: &mut Vec<ChatMessage>,
    agent_type: AgentType,
    turn_ctx: &TurnContext,
    tool_calls: &[novel_deepseek::LlmToolCall],
    completion: &LlmCompletion,
    results: Vec<(
        String,
        Result<novel_tools::ToolOutput, novel_tools::ToolError>,
    )>,
    event_tx: Option<&mpsc::UnboundedSender<Event>>,
    subs: &ForkStreamSubscriptions,
) -> Result<(), AgentError> {
    let result_ids: std::collections::HashSet<String> =
        results.iter().map(|(id, _)| id.clone()).collect();
    let missing: Vec<&str> = tool_calls
        .iter()
        .filter(|tc| !result_ids.contains(&tc.id))
        .map(|tc| tc.id.as_str())
        .collect();
    if !missing.is_empty() {
        tracing::debug!(
            ?agent_type,
            inner_turn = turn_ctx.inner_turn,
            ?missing,
            executor_result_count = results.len(),
            "subagent_executor_missing_tool_results"
        );
    }
    fork_child_push(
        db,
        fork_run_id,
        child_messages,
        crate::message::assistant_from_completion(completion),
    )?;
    let spec_by_id: std::collections::HashMap<String, ToolCallSpec> = tool_calls
        .iter()
        .map(|tc| {
            (
                tc.id.clone(),
                ToolCallSpec {
                    id: tc.id.clone(),
                    name: tc.name.clone(),
                    input: parse_tool_call_input(&tc.arguments, &tc.id, &tc.name),
                },
            )
        })
        .collect();
    for (id, result) in results {
        let spec = spec_by_id.get(&id);
        let content = format_tool(registry, spec, result).content;
        if let Some(tx) = event_tx {
            try_send_fork_overlay_event(
                subs,
                tx,
                Event::SubAgentToolUpdate {
                    fork_run_id: fork_run_id.to_string(),
                    phase: "result".into(),
                    tool_call_id: id.clone(),
                    tool_name: spec.map(|s| s.name.clone()),
                    input: None,
                    content: Some(content.clone()),
                    needs_approval: None,
                    status: None,
                    description: None,
                },
            );
        }
        fork_child_push(
            db,
            fork_run_id,
            child_messages,
            tool_result_message(&id, &content),
        )?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::fork_stream_subs::new_fork_stream_subscriptions;
    use novel_deepseek::{ContentBlockKind, LlmToolCall, StreamEvent};
    use novel_tools::{default_registry, PermissionMode};
    use std::sync::Arc;
    use tempfile::TempDir;
    use tokio::sync::mpsc;

    #[test]
    fn subagent_fork_tool_context_has_no_read_cache() {
        let tmp = tempfile::TempDir::new().unwrap();
        std::fs::create_dir_all(tmp.path().join("skills")).unwrap();
        let cfg = crate::EngineConfig {
            project_root: tmp.path().to_path_buf(),
            settings_path: tmp.path().join("settings.json"),
            db_path: tmp.path().join("state.db"),
            skills_dir: tmp.path().join("skills"),
            global_config_path: tmp.path().join(".novel-agent/api_config.json"),
        };
        let engine = crate::AgentEngine::new(cfg).unwrap();
        let ctx = super::subagent_fork_tool_context(&engine.shared);
        assert!(ctx.read_file_cache.is_none());
        assert!(ctx.on_read_cache_path_touched.is_none());
        assert!(ctx.file_op_locks.is_some());
        assert!(!ctx.allow_fork);
    }

    #[test]
    fn forward_subagent_stream_event_gated_until_subscribed() {
        let subs = new_fork_stream_subscriptions();
        let (tx, mut rx) = mpsc::unbounded_channel();
        for _ in 0..5 {
            forward_subagent_stream_event(
                &subs,
                &tx,
                "fr-1",
                StreamEvent::ContentBlockDelta {
                    index: 0,
                    delta: "hi".into(),
                    kind: ContentBlockKind::Text,
                },
            );
        }
        assert!(rx.try_recv().is_err());
        subs.write().unwrap().insert("fr-1".into());
        forward_subagent_stream_event(
            &subs,
            &tx,
            "fr-1",
            StreamEvent::ContentBlockDelta {
                index: 0,
                delta: "hi".into(),
                kind: ContentBlockKind::Text,
            },
        );
        assert!(rx.try_recv().is_ok());
    }

    #[tokio::test(flavor = "current_thread")]
    async fn execute_subagent_tool_batch_offline_read() {
        let tmp = TempDir::new().unwrap();
        std::fs::write(tmp.path().join("sub.txt"), "body").unwrap();
        let reg = Arc::new(default_registry());
        let ctx = ToolContext {
            permission_mode: PermissionMode::Auto,
            project_root: tmp.path().to_path_buf(),
            ..ToolContext::new(tmp.path().to_path_buf())
        };
        let tc = LlmToolCall {
            id: "r-sub".into(),
            name: "Read".into(),
            arguments: r#"{"file_path":"sub.txt"}"#.into(),
        };
        let subs = new_fork_stream_subscriptions();
        let results = execute_subagent_tool_batch(
            &reg,
            &ctx,
            None,
            std::slice::from_ref(&tc),
            None,
            "fork-test",
            &subs,
        )
        .await
        .unwrap();
        assert_eq!(results.len(), 1);
        assert!(results[0].1.is_ok());
    }

    #[test]
    fn subagent_push_tool_results_persists_messages() {
        let tmp = TempDir::new().unwrap();
        std::fs::create_dir_all(tmp.path().join("skills")).unwrap();
        let engine = crate::AgentEngine::new(crate::EngineConfig {
            project_root: tmp.path().to_path_buf(),
            settings_path: tmp.path().join("settings.json"),
            db_path: tmp.path().join("state.db"),
            skills_dir: tmp.path().join("skills"),
            global_config_path: tmp.path().join(".novel-agent/api_config.json"),
        })
        .unwrap();
        let fork_run_id = engine
            .shared
            .session
            .db
            .create_fork_run(
                &engine.shared.session.id,
                1,
                "KnowledgeAuditor",
                "task",
                "test",
            )
            .unwrap();
        let mut child_messages = Vec::new();
        let tc = LlmToolCall {
            id: "tr1".into(),
            name: "Read".into(),
            arguments: r#"{"file_path":"sub.txt"}"#.into(),
        };
        let completion = novel_deepseek::LlmCompletion {
            content: Some("assistant".into()),
            reasoning_content: None,
            tool_calls: vec![tc.clone()],
            stop_reason: Some("tool_calls".into()),
            usage: None,
        };
        let turn_ctx = TurnContext::new(8);
        subagent_push_tool_results(
            &engine.shared.registry,
            &engine.shared.session.db,
            &fork_run_id,
            &mut child_messages,
            AgentType::KnowledgeAuditor,
            &turn_ctx,
            std::slice::from_ref(&tc),
            &completion,
            vec![(
                "tr1".into(),
                Ok(novel_tools::ToolOutput {
                    content: "ok".into(),
                    is_error: false,
                }),
            )],
            None,
            &engine.shared.fork_stream_subs,
        )
        .unwrap();
        assert!(child_messages.iter().any(|m| m.role == "tool"));
    }
}
