//! Unified async subagent queue, drain, and per-job ReAct runner.

use crate::fork_transcript;
use crate::hooks::tool_schemas_for_agent;
use crate::interrupt_finalize::{
    finalize_stream_cancel, FinalizeStreamCancelParams, ForkTranscriptSink,
};
use crate::message_bridge::{
    assistant_from_completion, parse_tool_call_input, to_llm_messages_traced, tool_result_message,
    RepairTraceContext,
};
use crate::session_llm::{
    apply_session_usage, build_chat_client, read_session_llm, SessionLlmSnapshot,
};
use crate::subagent_overflow::{
    build_partial_report, task_preview_120, OVERFLOW_KIND_INPUT_REJECTED,
    OVERFLOW_KIND_OUTPUT_TRUNCATED,
};
use crate::subagent_react::{
    react_limit_reminder_message, report_only_tool_rejection, SubagentLoopPhase,
};
use crate::turn::TurnContext;
use crate::turn_loop::StreamingToolDispatch;
use crate::{
    AgentEngine, AgentError, AgentType, ChatMessage, Event, ForkError, ForkedAgentContext,
    TerminalReason,
};
use novel_deepseek::{
    is_context_length_exceeded, is_output_truncated, ChatClient, LlmToolCall, StreamEvent,
    StreamOutcome,
};
use novel_tools::{
    format_tool_result_for_llm, PendingSubagentWork, PermissionMode, StreamingToolExecutor,
    ToolCallSpec, ToolContext,
};
use std::sync::atomic::Ordering;
use std::sync::{Arc, Mutex};
use tokio::sync::mpsc;

#[derive(Debug, Clone)]
pub enum SubagentJobKind {
    ToolFork { parent_tool_call_id: String },
    HookAuditor,
}

#[derive(Debug, Clone)]
pub struct SubagentJob {
    pub agent_type: AgentType,
    pub task: String,
    pub kind: SubagentJobKind,
}

impl SubagentJob {
    pub fn from_pending(work: PendingSubagentWork) -> Result<Self, AgentError> {
        let agent_type = AgentType::parse(&work.agent_type).ok_or_else(|| {
            AgentError::Validation(format!("unknown fork agentType: {}", work.agent_type))
        })?;
        let kind = match work.parent_tool_call_id {
            Some(parent_tool_call_id) => SubagentJobKind::ToolFork {
                parent_tool_call_id,
            },
            None => SubagentJobKind::HookAuditor,
        };
        Ok(Self {
            agent_type,
            task: work.task,
            kind,
        })
    }

    pub fn inject_report(&self) -> bool {
        matches!(self.kind, SubagentJobKind::ToolFork { .. })
    }
}

pub fn build_fork_child(
    shared: &crate::EngineShared,
    agent_type: AgentType,
    task: String,
) -> Result<ForkedAgentContext, AgentError> {
    let system_msg = ChatMessage {
        role: "system".into(),
        content: shared.system_prompt.clone(),
        tool_call_id: None,
        tool_calls: None,
        reasoning_content: None,
    };
    ForkedAgentContext::fork(
        &system_msg,
        shared.session.id.clone(),
        agent_type,
        task,
        agent_type.max_react_loops_for(&shared.settings.agent),
        std::collections::HashMap::new(),
        false,
    )
    .map_err(|e| {
        if e == ForkError::InvalidMaxReactLoops(0) {
            AgentError::NestedForkProhibited
        } else {
            AgentError::Fork(e)
        }
    })
}

pub fn clear_subagent_queue(shared: &crate::EngineShared) {
    if let Ok(mut guard) = shared.subagent_queue.lock() {
        guard.clear();
    }
}

pub async fn drain_subagent_jobs(
    engine: &mut AgentEngine,
    event_tx: Option<&mpsc::UnboundedSender<Event>>,
) -> Result<(), AgentError> {
    let jobs: Vec<SubagentJob> = {
        let mut guard = engine
            .shared
            .subagent_queue
            .lock()
            .map_err(|_| AgentError::Validation("subagent queue lock poisoned".into()))?;
        std::mem::take(&mut *guard)
            .into_iter()
            .map(SubagentJob::from_pending)
            .collect::<Result<Vec<_>, _>>()?
    };
    if jobs.is_empty() {
        return Ok(());
    }

    let llm_snap = read_session_llm(&engine.shared);
    let hook_batch = jobs
        .iter()
        .any(|j| matches!(j.kind, SubagentJobKind::HookAuditor));
    if hook_batch {
        engine
            .shared
            .audit_log(&novel_logging::LogEvent::KnowledgeAuditorHookForked {
                session_id: engine.shared.session.id.clone(),
                trigger_tool: format!(
                    "batch: {} hook job(s)",
                    jobs.iter()
                        .filter(|j| matches!(j.kind, SubagentJobKind::HookAuditor))
                        .count()
                ),
            });
    }

    let saved_perm = engine.shared.permission_mode_override.clone();
    if hook_batch {
        if let Ok(mut g) = engine.shared.permission_mode_override.lock() {
            *g = PermissionMode::Auto;
        }
    }

    engine
        .shared
        .drain_in_progress
        .store(true, Ordering::SeqCst);
    struct DrainGuard(Arc<std::sync::atomic::AtomicBool>);
    impl Drop for DrainGuard {
        fn drop(&mut self) {
            self.0.store(false, Ordering::SeqCst);
        }
    }
    let _guard = DrainGuard(Arc::clone(&engine.shared.drain_in_progress));

    let mut handles = Vec::with_capacity(jobs.len());
    let mut meta: Vec<(AgentType, String, bool)> = Vec::with_capacity(jobs.len());
    for job in jobs {
        let agent_type = job.agent_type;
        let inject = job.inject_report();
        let fork_run_id = fork_transcript::create_fork_run(
            &engine.shared.session.db,
            &engine.shared.session.id,
            engine.turn_number as i32,
            &agent_type.to_string(),
            &job.task,
            if inject { "tool" } else { "hook" },
        )?;
        let shared = engine.shared.clone();
        let snap = llm_snap.clone();
        let job_for_spawn = job.clone();
        engine.sub_agent_inc();
        let event_tx_clone = event_tx.cloned();
        meta.push((agent_type, fork_run_id.clone(), inject));
        handles.push(tokio::spawn(async move {
            let result =
                run_subagent_job(shared, job_for_spawn, fork_run_id, snap, event_tx_clone).await;
            (agent_type, result)
        }));
    }

    for (i, handle) in handles.into_iter().enumerate() {
        let (_agent_type, fork_run_id, inject) =
            meta.get(i)
                .cloned()
                .unwrap_or((AgentType::KnowledgeAuditor, String::new(), false));
        match handle.await {
            Ok((at, Ok(output))) => {
                if inject {
                    engine.inject_sub_agent_report(at, &output, Some(&fork_run_id))?;
                    if engine.compaction_needed() {
                        engine.compact_with_events(event_tx).await;
                    }
                } else {
                    let _ = fork_transcript::finish_fork_run(
                        &engine.shared.session.db,
                        &fork_run_id,
                        "complete",
                        None,
                    );
                }
            }
            Ok((_, Err(e))) => {
                tracing::error!(error = %e, "subagent_job_failed");
            }
            Err(e) => {
                return Err(AgentError::Validation(format!(
                    "subagent task join failed: {e}"
                )));
            }
        }
    }

    if hook_batch {
        if let Ok(mut g) = saved_perm.lock() {
            *g = PermissionMode::Normal;
        }
    }
    Ok(())
}
/// Run one subagent job (tool fork or PostToolUse hook). Transcript -> `fork_messages` only.
pub async fn run_subagent_job(
    shared: crate::EngineShared,
    job: SubagentJob,
    fork_run_id: String,
    llm_snap: SessionLlmSnapshot,
    event_tx: Option<mpsc::UnboundedSender<Event>>,
) -> Result<String, AgentError> {
    let agent_type = job.agent_type;
    let task = job.task;
    let parent_tool_call_id = match job.kind {
        SubagentJobKind::ToolFork {
            parent_tool_call_id,
        } => Some(parent_tool_call_id),
        SubagentJobKind::HookAuditor => None,
    };
    tracing::debug!(
        session_id = %shared.session.id,
        ?agent_type,
        task_len = task.len(),
        "subagent_job_start"
    );
    let mut llm = build_chat_client(&llm_snap, &shared.global_config_path);
    let mut child = build_fork_child(&shared, agent_type, task.clone())?;

    let task_preview: String = task.chars().take(80).collect();
    let task_preview = if task.chars().count() > 80 {
        format!("{task_preview}…")
    } else {
        task_preview
    };
    if let Some(ref tx) = event_tx {
        let _ = tx.send(Event::SubAgentStarted {
            fork_run_id: fork_run_id.clone(),
            agent_id: agent_type.to_string(),
            agent_type: agent_type.to_string(),
            task_preview,
            parent_tool_call_id: parent_tool_call_id.clone(),
        });
    }
    crate::fork_transcript::persist_fork_message(
        &shared.session.db,
        &fork_run_id,
        &child.fork.task_message,
    )?;

    // Sub-agent inner loop
    let allowed = child.fork.agent_def.tools.clone();
    let schemas = tool_schemas_for_agent(&shared.registry, &allowed);
    let max_react_loops = child.fork.max_react_loops;

    let mut turn_ctx = TurnContext::new(1, max_react_loops);
    let mut phase = SubagentLoopPhase::Reacting;
    let mut was_interrupted = false;
    let mut interrupt_reason = "";
    loop {
        if shared.abort_controller.is_aborted() {
            was_interrupted = true;
            interrupt_reason = "用户取消";
            break;
        }
        if matches!(phase, SubagentLoopPhase::Reacting) && !turn_ctx.needs_continuation() {
            let spent = turn_ctx.inner_spent();
            let reminder = react_limit_reminder_message(spent, max_react_loops);
            fork_child_push(
                &shared.session.db,
                &fork_run_id,
                &mut child.messages,
                reminder,
            )?;
            phase = phase.enter_report_only();
        }
        tracing::debug!(
            session_id = %shared.session.id,
            %fork_run_id,
            ?agent_type,
            inner_turn = turn_ctx.inner_turn,
            message_count = child.messages.len(),
            phase = %if phase.is_report_only() { "report_only" } else { "reacting" },
            "subagent_inner_turn_start"
        );
        let llm_msgs = to_llm_messages_traced(
            &child.messages,
            Some(RepairTraceContext {
                label: "subagent_job",
                fork_run_id: Some(&fork_run_id),
                inner_turn: Some(turn_ctx.inner_turn),
                session_id: Some(&shared.session.id),
            }),
        );
        let snapshot = child.messages.clone();
        let active_schemas: &[(String, String, serde_json::Value)] = if phase.is_report_only() {
            &[]
        } else {
            &schemas[..]
        };

        let mut fork_dispatch: Option<Arc<Mutex<StreamingToolDispatch>>> = None;
        let fork_ctx = ToolContext {
            permission_mode: match shared.settings.permissions.mode.as_str() {
                "plan" => PermissionMode::Plan,
                "auto" => PermissionMode::Auto,
                "unattended" => PermissionMode::Unattended,
                _ => PermissionMode::Normal,
            },
            deny_rules: shared.settings.permissions.deny_rules.clone(),
            always_allow: crate::agent::merge_tool_always_allow(
                &shared.settings.permissions.always_allow,
            ),
            project_root: shared.session.project_root.clone(),
            session_id: shared.session.id.clone(),
            db: Some(Arc::new(shared.session.db.clone())),
            permission_mode_override: Some(Arc::clone(&shared.permission_mode_override)),
            read_file_cache: Some(Arc::clone(&shared.read_file_cache)),
            allow_fork: false,
            subagent_queue: None,
            current_tool_call_id: None,
            skills_dir: Some(shared.agent_skills_dir.clone()),
            global_api_config_path: Some(shared.global_config_path.clone()),
        };

        let completion = if let Some(ref mut client) = llm {
            let cancel_flag = shared.abort_controller.cancel_flag();
            let (abort_tx, abort_rx) = novel_tools::abort_channel();
            let dispatch_arc = Arc::new(Mutex::new(StreamingToolDispatch::new(
                Arc::clone(&shared.registry),
                fork_ctx.clone(),
                1,
                abort_rx,
            )));
            fork_dispatch = Some(Arc::clone(&dispatch_arc));
            let dispatch_cb = Arc::clone(&dispatch_arc);
            let registry_cb = Arc::clone(&shared.registry);
            let ctx_cb = fork_ctx.clone();
            let on_tool = move |tc: LlmToolCall| {
                if let Ok(mut d) = dispatch_cb.lock() {
                    d.handle_ready(&registry_cb, &ctx_cb, None, tc, false);
                }
            };
            let _ = abort_tx;
            let fork_run_id_stream = fork_run_id.clone();
            let event_tx_stream = event_tx.clone();
            let result = client
                .create_stream(
                    &llm_msgs,
                    active_schemas,
                    shared.settings.model.max_output_tokens,
                    move |ev: StreamEvent| {
                        if let Some(ref tx) = event_tx_stream {
                            match ev {
                                StreamEvent::ContentBlockDelta { delta, kind, .. } => {
                                    let _ = tx.send(Event::SubAgentStreamDelta {
                                        fork_run_id: fork_run_id_stream.clone(),
                                        delta,
                                        kind,
                                    });
                                }
                                StreamEvent::ToolUseStarted {
                                    tool_call_id, name, ..
                                } => {
                                    let _ = tx.send(Event::SubAgentToolUpdate {
                                        fork_run_id: fork_run_id_stream.clone(),
                                        phase: "start".into(),
                                        tool_call_id,
                                        tool_name: Some(name),
                                        input: None,
                                        content: None,
                                        needs_approval: None,
                                        status: None,
                                        description: None,
                                    });
                                }
                                StreamEvent::ToolInputDelta {
                                    tool_call_id,
                                    delta,
                                } => {
                                    let _ = tx.send(Event::SubAgentToolUpdate {
                                        fork_run_id: fork_run_id_stream.clone(),
                                        phase: "input_delta".into(),
                                        tool_call_id,
                                        tool_name: None,
                                        input: None,
                                        content: Some(delta),
                                        needs_approval: None,
                                        status: None,
                                        description: None,
                                    });
                                }
                                StreamEvent::MessageStop { .. }
                                | StreamEvent::StreamError { .. } => {}
                            }
                        }
                    },
                    Some(on_tool),
                    Some(cancel_flag),
                )
                .await;
            match result {
                Ok(StreamOutcome::Complete(c)) => c,
                Ok(StreamOutcome::Cancelled {
                    partial,
                    background_usage,
                }) => {
                    if let Ok(mut d) = dispatch_arc.lock() {
                        d.discard();
                    }
                    was_interrupted = true;
                    interrupt_reason = "流中断";
                    tracing::debug!(
                        session_id = %shared.session.id,
                        %fork_run_id,
                        ?agent_type,
                        inner_turn = turn_ctx.inner_turn,
                        message_count = child.messages.len(),
                        partial_tool_call_count = partial.tool_calls.len(),
                        "subagent_stream_cancelled"
                    );
                    let mut sink = ForkTranscriptSink {
                        db: &shared.session.db,
                        fork_run_id: &fork_run_id,
                        child: &mut child.messages,
                    };
                    finalize_stream_cancel(FinalizeStreamCancelParams {
                        sink: &mut sink,
                        partial,
                        llm_messages: llm_msgs.clone(),
                        tool_schemas: active_schemas.to_vec(),
                        background_usage: Some(background_usage),
                        llm_snap: llm_snap.clone(),
                        shared: shared.clone(),
                        event_tx: event_tx.as_ref(),
                    })
                    .await?;
                    break;
                }
                Err(e) if is_context_length_exceeded(&e) => {
                    tracing::warn!(
                        session_id = %shared.session.id,
                        ?agent_type,
                        error = %e,
                        "subagent_context_length_exceeded"
                    );
                    if let Ok(mut d) = dispatch_arc.lock() {
                        d.discard();
                    }
                    child.messages = snapshot;
                    shared
                        .sub_agent_count
                        .fetch_sub(1, std::sync::atomic::Ordering::SeqCst);
                    return Ok(build_partial_report(
                        &agent_type.to_string(),
                        &task_preview_120(&task),
                        OVERFLOW_KIND_INPUT_REJECTED,
                    ));
                }
                Err(e) => {
                    tracing::warn!(
                        session_id = %shared.session.id,
                        ?agent_type,
                        error = %e,
                        "subagent_llm_error"
                    );
                    if let Ok(mut d) = dispatch_arc.lock() {
                        d.discard();
                    }
                    was_interrupted = true;
                    interrupt_reason = "API error";
                    break;
                }
            }
        } else {
            ChatClient::offline_complete(&llm_msgs)
        };

        if is_output_truncated(completion.stop_reason.as_deref()) {
            tracing::warn!(
                session_id = %shared.session.id,
                %fork_run_id,
                ?agent_type,
                inner_turn = turn_ctx.inner_turn,
                tool_call_count = completion.tool_calls.len(),
                "subagent_output_truncated"
            );
            fork_child_push(
                &shared.session.db,
                &fork_run_id,
                &mut child.messages,
                assistant_from_completion(&completion),
            )?;
            shared
                .sub_agent_count
                .fetch_sub(1, std::sync::atomic::Ordering::SeqCst);
            return Ok(build_partial_report(
                &agent_type.to_string(),
                &task_preview_120(&task),
                OVERFLOW_KIND_OUTPUT_TRUNCATED,
            ));
        }

        if let Some(u) = &completion.usage {
            apply_session_usage(&shared, u, &llm_snap, event_tx.as_ref());
        }

        if let Some(ref tx) = event_tx {
            let _ = tx.send(Event::AssistantSegmentComplete {
                segment_index: turn_ctx.inner_turn,
                fork_run_id: Some(fork_run_id.clone()),
            });
        }

        let tool_calls = completion.tool_calls.clone();
        if tool_calls.is_empty() {
            fork_child_push(
                &shared.session.db,
                &fork_run_id,
                &mut child.messages,
                assistant_from_completion(&completion),
            )?;
            break;
        }

        if phase.is_report_only() {
            fork_child_push(
                &shared.session.db,
                &fork_run_id,
                &mut child.messages,
                assistant_from_completion(&completion),
            )?;
            for tc in &tool_calls {
                fork_child_push(
                    &shared.session.db,
                    &fork_run_id,
                    &mut child.messages,
                    report_only_tool_rejection(&tc.id),
                )?;
            }
            if let Some(next) = phase.consume_grace() {
                phase = next;
                continue;
            }
            was_interrupted = true;
            interrupt_reason = "报告收尾失败";
            break;
        }

        // Execute tools inline (sub-agent; streaming dispatch when LLM available)
        if let Some(ref tx) = event_tx {
            for tc in &tool_calls {
                let input = parse_tool_call_input(&tc.arguments, &tc.id, &tc.name);
                let _ = tx.send(Event::SubAgentToolUpdate {
                    fork_run_id: fork_run_id.clone(),
                    phase: "input_complete".into(),
                    tool_call_id: tc.id.clone(),
                    tool_name: Some(tc.name.clone()),
                    input: Some(input),
                    content: None,
                    needs_approval: None,
                    status: None,
                    description: None,
                });
            }
        }
        let results = if let Some(dispatch_arc) = fork_dispatch {
            let mut executor = {
                let mut dispatch = dispatch_arc.lock().map_err(|_| {
                    AgentError::Validation("fork tool dispatch lock poisoned".into())
                })?;
                for tc in &tool_calls {
                    if !dispatch.handled_ids.contains(&tc.id) {
                        dispatch.handle_ready(&shared.registry, &fork_ctx, None, tc.clone(), true);
                    }
                }
                dispatch.take_executor()
            };
            executor.get_remaining_results().await
        } else {
            let mut executor = StreamingToolExecutor::new(
                Arc::clone(&shared.registry),
                fork_ctx.clone(),
                1,
                novel_tools::abort_channel().1,
            );
            for tc in &tool_calls {
                let spec = ToolCallSpec {
                    id: tc.id.clone(),
                    name: tc.name.clone(),
                    input: parse_tool_call_input(&tc.arguments, &tc.id, &tc.name),
                };
                if shared.registry.get(&spec.name).is_some() {
                    executor.add_tool(spec);
                }
            }
            executor.get_remaining_results().await
        };
        let result_ids: std::collections::HashSet<String> =
            results.iter().map(|(id, _)| id.clone()).collect();
        let missing_from_executor: Vec<&str> = tool_calls
            .iter()
            .filter(|tc| !result_ids.contains(&tc.id))
            .map(|tc| tc.id.as_str())
            .collect();
        if !missing_from_executor.is_empty() {
            tracing::debug!(
                session_id = %shared.session.id,
                %fork_run_id,
                ?agent_type,
                inner_turn = turn_ctx.inner_turn,
                ?missing_from_executor,
                executor_result_count = results.len(),
                "subagent_executor_missing_tool_results"
            );
        }
        fork_child_push(
            &shared.session.db,
            &fork_run_id,
            &mut child.messages,
            assistant_from_completion(&completion),
        )?;
        tracing::debug!(
            session_id = %shared.session.id,
            %fork_run_id,
            ?agent_type,
            inner_turn = turn_ctx.inner_turn,
            tool_call_count = tool_calls.len(),
            tool_call_ids = ?tool_calls.iter().map(|tc| tc.id.as_str()).collect::<Vec<_>>(),
            message_count_after_assistant = child.messages.len(),
            "subagent_assistant_pushed_before_tool_results"
        );
        let fork_spec_by_id: std::collections::HashMap<String, ToolCallSpec> = tool_calls
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
        let tool_results_count = results.len();
        for (id, result) in results {
            let spec = fork_spec_by_id.get(&id);
            let content = format_tool(spec, result).content;
            if let Some(ref tx) = event_tx {
                let _ = tx.send(Event::SubAgentToolUpdate {
                    fork_run_id: fork_run_id.clone(),
                    phase: "result".into(),
                    tool_call_id: id.clone(),
                    tool_name: spec.map(|s| s.name.clone()),
                    input: None,
                    content: Some(content.clone()),
                    needs_approval: None,
                    status: None,
                    description: None,
                });
            }
            fork_child_push(
                &shared.session.db,
                &fork_run_id,
                &mut child.messages,
                tool_result_message(&id, &content),
            )?;
        }
        tracing::debug!(
            session_id = %shared.session.id,
            %fork_run_id,
            ?agent_type,
            inner_turn = turn_ctx.inner_turn,
            tool_results_pushed = tool_results_count,
            message_count_after_tools = child.messages.len(),
            "subagent_tool_results_pushed"
        );

        if let Err(TerminalReason::MaxReactLoops(_)) = turn_ctx.increment_inner() {
            let spent = turn_ctx.inner_spent();
            fork_child_push(
                &shared.session.db,
                &fork_run_id,
                &mut child.messages,
                react_limit_reminder_message(spent, max_react_loops),
            )?;
            phase = phase.enter_report_only();
            continue;
        }
    }

    let last_assistant = child
        .messages
        .iter()
        .rev()
        .find(|m| m.role == "assistant")
        .map(|m| m.content.clone())
        .unwrap_or_else(|| "（无文本输出）".into());

    let output = if was_interrupted {
        let task_preview: String = child.fork.task_message.content.chars().take(200).collect();
        let task_preview = if child.fork.task_message.content.len() > 200 {
            format!("{task_preview}…")
        } else {
            task_preview
        };
        format!(
            "[子 Agent 已中断: {}]\n\n\
             ⚠ 用户中断。部分修改可能已写入磁盘，KB 可能处于不一致状态。\n\n\
             - 任务: {}\n\
             - 已执行轮数: {} / {} 轮\n\
             - 中断原因: {}\n\n\
             ## 最后输出\n\n{}\n\n\
             ## 说明\n\
             请根据最后输出和已执行轮数判断后续操作。如有文件写入，建议验证一致性后再继续。",
            agent_type,
            task_preview,
            turn_ctx.inner_turn,
            max_react_loops,
            interrupt_reason,
            last_assistant,
        )
    } else {
        last_assistant
    };

    // Decrement sub-agent count
    shared
        .sub_agent_count
        .fetch_sub(1, std::sync::atomic::Ordering::SeqCst);

    if let Some(ref tx) = event_tx {
        let _ = tx.send(Event::SubAgentComplete {
            fork_run_id: fork_run_id.clone(),
            agent_id: agent_type.to_string(),
            output: output.clone(),
            cache_hit_rate: 0.0,
        });
    }

    tracing::debug!(
        session_id = %shared.session.id,
        ?agent_type,
        output_len = output.len(),
        was_interrupted,
        inner_turns = turn_ctx.inner_turn,
        "subagent_job_complete"
    );

    Ok(output)
}

fn fork_child_push(
    db: &novel_state::Database,
    run_id: &str,
    child: &mut Vec<ChatMessage>,
    msg: ChatMessage,
) -> Result<(), AgentError> {
    crate::fork_transcript::persist_fork_message(db, run_id, &msg)?;
    child.push(msg);
    Ok(())
}

fn format_tool(
    spec: Option<&ToolCallSpec>,
    result: Result<novel_tools::ToolOutput, novel_tools::ToolError>,
) -> novel_tools::FormattedToolResult {
    let spec_ref = spec
        .map(|s| novel_tools::ToolResultSpec {
            tool_name: &s.name,
            tool_input: Some(&s.input),
        })
        .unwrap_or(novel_tools::ToolResultSpec {
            tool_name: "",
            tool_input: None,
        });
    format_tool_result_for_llm(spec_ref, result)
}
