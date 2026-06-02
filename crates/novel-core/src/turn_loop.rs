use crate::dynamic_context::{
    filter_loadable_reference_paths, filter_loadable_skill_ids, format_activated_skill_block,
    parse_skill_reference_path,
};
use crate::interrupt::{AbortController, InterruptReason, ERROR_MESSAGE_USER_ABORT};
use crate::interrupt_finalize::{
    finalize_stream_cancel, FinalizeStreamCancelParams, MainSessionSink,
};
use crate::message_bridge::{
    assistant_from_completion, chat_slice_to_compaction, chat_to_compaction, chat_to_json,
    compaction_slice_to_chat, parse_tool_call_input, to_llm_messages, to_llm_messages_traced,
    tool_result_message, RepairTraceContext,
};
use crate::session_llm::{
    apply_session_usage, build_chat_client, read_session_llm, write_session_llm, SessionLlmSnapshot,
};
use crate::subagent::{clear_subagent_queue, drain_subagent_jobs};
use crate::turn::TurnContext;
use crate::turn::MSG_SEQ_USER;
use crate::{
    hooks::tool_schemas_for_agent, AgentEngine, AgentError, AgentType, ChatMessage,
    CompactionAction, ContentBlockKind, Event, TerminalReason,
};
use novel_deepseek::{
    is_output_truncated, ChatClient, LlmChatMessage, LlmCompletion, LlmError, LlmToolCall,
    StreamEvent, StreamOutcome,
};
use novel_logging::LogEvent;
use novel_tools::{
    AbortSignal, PendingSubagentWork, PermissionResult, StreamingToolExecutor, ToolCallSpec,
    ToolContext, ToolExecutor, ToolRegistry,
};
use std::collections::{HashMap, HashSet};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;
use tokio::sync::{mpsc, watch};

const MAIN_MAX_INNER_TURNS: u32 = 80;

/// DeepSeek thinking mode often ends a stream with only `reasoning_content` and no
/// `tool_calls`, while the model still intends to act on the next inner iteration.
fn should_continue_inner_after_completion(completion: &LlmCompletion) -> bool {
    if !completion.tool_calls.is_empty() {
        return false;
    }
    if is_output_truncated(completion.stop_reason.as_deref()) {
        return false;
    }
    let content_empty = completion
        .content
        .as_ref()
        .map(|s| s.trim().is_empty())
        .unwrap_or(true);
    let has_reasoning = completion
        .reasoning_content
        .as_ref()
        .map(|s| !s.trim().is_empty())
        .unwrap_or(false);
    content_empty && has_reasoning
}

fn map_abort_signal(reason: Option<InterruptReason>) -> AbortSignal {
    match reason {
        Some(InterruptReason::UserCancel) => AbortSignal::UserCancel,
        Some(InterruptReason::SubmitInterrupt) => AbortSignal::SubmitInterrupt,
        Some(InterruptReason::SiblingError) => AbortSignal::SiblingError,
        Some(InterruptReason::StreamingFallback) => AbortSignal::StreamingFallback,
        None => AbortSignal::None,
    }
}

async fn run_abort_bridge(ac: Arc<AbortController>, tx: watch::Sender<AbortSignal>) {
    let mut rx = ac.subscribe();
    loop {
        if rx.changed().await.is_err() {
            break;
        }
        let _ = tx.send(map_abort_signal(*rx.borrow()));
    }
}

enum LlmCallOutcome {
    Continue(LlmCompletion),
    Aborted(TerminalReason),
}

pub(crate) struct StreamingToolDispatch {
    executor: Option<StreamingToolExecutor>,
    pub(crate) handled_ids: HashSet<String>,
    executed_specs: Vec<ToolCallSpec>,
    pending_specs: HashMap<String, ToolCallSpec>,
    denied_specs: HashMap<String, (ToolCallSpec, String)>,
    ui_result_emitted: HashSet<String>,
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

    fn executor_mut(&mut self) -> &mut StreamingToolExecutor {
        self.executor
            .as_mut()
            .expect("streaming tool executor already taken")
    }

    pub(crate) fn take_executor(&mut self) -> StreamingToolExecutor {
        self.executor
            .take()
            .expect("streaming tool executor already taken")
    }

    /// Move tools that were streamed and queued for approval into the engine's `pending_tools`.
    fn drain_pending_specs(&mut self, engine: &mut AgentEngine) {
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
        let validation_err: Option<Result<(), String>> = if tc.arguments.trim().is_empty() {
            registry
                .get(&tc.name)
                .map(|t| t.validate_input(&input).map_err(|e| e.to_string()))
        } else if input.as_object().is_some_and(|o| o.is_empty()) {
            Some(Err(format!("Invalid tool arguments JSON for {}", tc.name)))
        } else {
            registry
                .get(&tc.name)
                .map(|t| t.validate_input(&input).map_err(|e| e.to_string()))
        };

        if let Some(Err(reason)) = validation_err {
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
            self.handled_ids.insert(tc.id.clone());
            let spec = ToolCallSpec {
                id: tc.id.clone(),
                name: tc.name.clone(),
                input,
            };
            self.denied_specs.insert(spec.id.clone(), (spec, reason));
            return;
        }
        if registry.get(&tc.name).is_none() {
            if !finalize {
                return;
            }
            self.handled_ids.insert(tc.id.clone());
            let spec = ToolCallSpec {
                id: tc.id.clone(),
                name: tc.name.clone(),
                input,
            };
            self.denied_specs
                .insert(spec.id.clone(), (spec, "unknown tool".into()));
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
                self.executor_mut().add_tool(spec);
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

    fn poll_ui_results(&mut self, event_tx: Option<&mpsc::UnboundedSender<Event>>) {
        // Must not drain `completed` — results are collected once in get_remaining_results
        // for persistence and the next LLM call. Draining here caused UI success + missing
        // tool_result messages (model reports "timeout" / retries InvokeSkill).
        let completed = self.executor_mut().peek_completed_results();
        let peek_count = completed.len();
        if peek_count > 0 {
            tracing::debug!(peek_count, "poll_ui_results peeked completed tool results");
        }
        for (id, result) in completed {
            if !self.ui_result_emitted.insert(id.clone()) {
                continue;
            }
            let spec = self.executed_specs.iter().find(|s| s.id == id);
            let content = format_tool(spec, result).content;
            if let Some(tx) = event_tx {
                let _ = tx.send(Event::ToolCallResult {
                    tool_call_id: id,
                    content,
                });
            }
        }
    }

    pub(crate) fn discard(&mut self) {
        if let Some(executor) = self.executor.as_mut() {
            executor.discard();
        }
    }
}

impl AgentEngine {
    /// Lazily build main-session `ChatClient` from [`read_session_llm`] + [`build_chat_client`].
    /// No-op when `self.llm` is already set (e.g. per-turn model override rebuilt the client).
    pub fn init_llm(&mut self) {
        if self.llm.is_some() {
            return;
        }
        let snap = read_session_llm(&self.shared);
        self.llm = build_chat_client(&snap, &self.shared.global_config_path);
        self.sync_session_llm_from_llm();
    }

    /// Prefix for sub-agent reports injected mid-turn (role stays `user` for the LLM).
    const SUB_AGENT_REPORT_PREFIX: &'static str = "[子 Agent 完成:";

    fn is_sub_agent_report(msg: &ChatMessage) -> bool {
        msg.content.starts_with(Self::SUB_AGENT_REPORT_PREFIX)
    }

    // ── Main agent turn ───────────────────────────────────────

    pub async fn handle_message_with_events(
        &mut self,
        content: &str,
        model_override: Option<&str>,
        event_tx: Option<&mpsc::UnboundedSender<Event>>,
    ) -> Result<TerminalReason, AgentError> {
        if content.trim().is_empty() {
            tracing::warn!("handle_message rejected: empty content");
            return Err(AgentError::Validation("empty message".into()));
        }
        if self.pending_user_question.is_some() {
            tracing::warn!(
                session_id = %self.shared.session.id,
                "handle_message rejected: pending user question"
            );
            return Err(AgentError::Validation(
                "answer pending question before sending a new message".into(),
            ));
        }
        self.clear_interrupt();
        self.turn_number += 1;

        let _ = self
            .shared
            .session
            .db
            .sync_user_turn_count(&self.shared.session.id, self.turn_number as i32);

        // Set session title from first user message
        if self.turn_number == 1 {
            let title: String = content.chars().take(50).collect();
            let _ = self
                .shared
                .session
                .db
                .set_session_title(&self.shared.session.id, &title);
        }
        // Per-turn model snapshot (StatusBar override; does not write settings.json).
        let turn_snap = match model_override {
            Some(m) if !m.is_empty() && m != self.shared.settings.model.model => {
                SessionLlmSnapshot {
                    model: m.to_string(),
                    thinking_enabled: self.shared.settings.model.thinking_enabled,
                }
            }
            _ => SessionLlmSnapshot::from_settings(&self.shared.settings),
        };
        write_session_llm(&self.shared, turn_snap.clone());
        let per_turn_model_override =
            model_override.is_some_and(|m| !m.is_empty() && m != self.shared.settings.model.model);
        // Must rebuild when override changes: `init_llm` skips if `self.llm` is already set.
        if per_turn_model_override {
            self.llm = build_chat_client(&turn_snap, &self.shared.global_config_path);
            self.sync_session_llm_from_llm();
        }

        self.pending_tools.clear();
        let user_msg = ChatMessage {
            role: "user".into(),
            content: content.to_string(),
            tool_call_id: None,
            tool_calls: None,
            reasoning_content: None,
        };
        self.messages.push(user_msg.clone());
        self.turn_message_seq = 0;
        self.persist_message_at_seq(&user_msg, MSG_SEQ_USER)?;

        let session_id = self.shared.session.id.clone();
        let message_count = self.messages.len();
        tracing::info!(turn = self.turn_number, "turn_start");
        tracing::debug!(
            session_id = %session_id,
            message_count,
            content_len = content.len(),
            "turn_start_detail"
        );
        self.audit_log(LogEvent::TurnStarted {
            session_id: session_id.clone(),
            turn_number: self.turn_number,
            message_count,
        });

        if let Some(tx) = event_tx {
            let _ = tx.send(Event::TurnStart {
                turn_number: self.turn_number,
            });
        }

        self.init_llm();

        let mut turn_ctx = TurnContext::new(self.turn_number, MAIN_MAX_INNER_TURNS);
        let reason = self.run_inner_turn_loop(&mut turn_ctx, event_tx).await?;

        let (hit, miss, comp, _ctx) = self.session_token_summary();
        let (th, tm, tc) = self
            .last_turn_usage
            .as_ref()
            .map(|u| (u.cache_hit_tokens, u.cache_miss_tokens, u.completion_tokens))
            .unwrap_or((0, 0, 0));
        tracing::info!(turn = self.turn_number, ?reason, "turn_complete");
        tracing::debug!(
            session_id = %session_id,
            cache_hit = hit,
            cache_miss = miss,
            completion = comp,
            turn_hit = th,
            turn_miss = tm,
            turn_comp = tc,
            "turn_complete_detail"
        );
        self.audit_log(LogEvent::TurnCompleted {
            session_id: session_id.clone(),
            turn_number: self.turn_number,
            cache_hit_tokens: hit,
            cache_miss_tokens: miss,
            completion_tokens: comp,
        });

        let turn_paused = self.pending_user_question.is_some() || !self.pending_tools.is_empty();
        if let Some(tx) = event_tx {
            if reason.is_aborted() {
                self.audit_error("用户已中断", true);
            }
            // Paused for AskUserQuestion or tool approval — not a finished turn.
            if !turn_paused {
                let _ = tx.send(Event::TurnComplete {
                    turn_number: self.turn_number,
                    cache_hit_tokens: hit,
                    cache_miss_tokens: miss,
                    completion_tokens: comp,
                    turn_hit_tokens: th,
                    turn_miss_tokens: tm,
                    turn_comp_tokens: tc,
                    was_interrupted: reason.is_aborted(),
                });
            }
        }
        self.clear_interrupt();
        Ok(reason)
    }

    /// Inject sub-agent report into the parent session so the main LLM can see it.
    /// Only the summary enters `self.messages`; full transcript stays in `fork_messages`.
    pub fn inject_sub_agent_report(
        &mut self,
        agent_type: AgentType,
        output: &str,
        fork_run_id: Option<&str>,
    ) -> Result<(), AgentError> {
        let msg = ChatMessage {
            role: "user".into(),
            content: format!("{} {agent_type}]\n{output}", Self::SUB_AGENT_REPORT_PREFIX),
            tool_call_id: None,
            tool_calls: None,
            reasoning_content: None,
        };
        tracing::debug!(
            ?agent_type,
            output_len = output.len(),
            ?fork_run_id,
            "inject_sub_agent_report"
        );
        // Must not reuse MSG_SEQ_USER (0): the real user message already occupies it.
        let extra = fork_run_id.map(|id| serde_json::json!({ "fork_run_id": id }));
        let report_id = self.persist_message_alloc_ex(&msg, extra.as_ref())?;
        self.messages.push(msg);
        if let Some(run_id) = fork_run_id {
            crate::fork_transcript::finish_fork_run(
                &self.shared.session.db,
                run_id,
                "complete",
                Some(&report_id),
            )?;
        }
        Ok(())
    }

    /// Next `TurnContext::inner_turn` so assistant DB sequence stays `inner_turn + 1`
    /// without colliding after `answer_question` / `approve_tool` / `deny_tool` resume.
    fn resume_inner_turn_from_messages(&self) -> u32 {
        self.messages
            .iter()
            .filter(|m| m.role == "assistant")
            .count() as u32
    }

    // ── Inner turn loop ───────────────────────────────────────

    async fn run_inner_turn_loop(
        &mut self,
        turn_ctx: &mut TurnContext,
        event_tx: Option<&mpsc::UnboundedSender<Event>>,
    ) -> Result<TerminalReason, AgentError> {
        loop {
            if self.interrupt_requested() {
                return Ok(TerminalReason::AbortedStreaming);
            }
            if turn_ctx.has_pending_approvals() || !self.pending_tools.is_empty() {
                return Ok(TerminalReason::Completed);
            }
            if !turn_ctx.needs_continuation() {
                return Ok(TerminalReason::MaxReactLoops(turn_ctx.max_inner_turns));
            }

            let schemas = tool_schemas_for_agent(&self.shared.registry, &self.main_tool_names());

            let llm_msgs = to_llm_messages_traced(
                &self.messages,
                Some(RepairTraceContext {
                    label: "main_inner",
                    fork_run_id: None,
                    inner_turn: Some(turn_ctx.inner_turn),
                    session_id: Some(&self.shared.session.id),
                }),
            );
            tracing::debug!(
                inner_turn = turn_ctx.inner_turn,
                message_count = self.messages.len(),
                llm_message_count = llm_msgs.len(),
                tool_schema_count = schemas.len(),
                "inner_turn_iteration"
            );

            let completion = match self
                .call_llm_and_execute(&llm_msgs, &schemas, turn_ctx, event_tx, true)
                .await?
            {
                LlmCallOutcome::Aborted(r) => {
                    clear_subagent_queue(&self.shared);
                    return Ok(r);
                }
                LlmCallOutcome::Continue(c) => c,
            };

            if self.pending_user_question.is_some() {
                // Assistant (+ tool stub) already persisted in call_llm_and_execute.
                return Ok(TerminalReason::Completed);
            }

            drain_subagent_jobs(self, event_tx).await?;

            // Update real context token count from API response.
            // Total = input (cache_hit + cache_miss) + output (completion_tokens).
            // The completion becomes part of messages and will be sent as input next call.
            if let Some(u) = &completion.usage {
                self.last_context_tokens =
                    (u.cache_hit_tokens + u.cache_miss_tokens + u.completion_tokens) as usize;
            }
            // Trigger compaction if real token count exceeds threshold
            if self.compaction_needed() {
                self.compact_with_events(event_tx).await;
            }

            if completion.tool_calls.is_empty() {
                let assistant = assistant_from_completion(&completion);
                self.persist_message_alloc(&assistant)?;
                self.messages.push(assistant);
                if self.interrupt_requested() {
                    return Ok(TerminalReason::AbortedStreaming);
                }
                if !self.pending_tools.is_empty() {
                    tracing::debug!(
                        pending_tool_count = self.pending_tools.len(),
                        "inner_turn_paused_pending_tool_approval"
                    );
                    return Ok(TerminalReason::Completed);
                }
                if should_continue_inner_after_completion(&completion) {
                    tracing::debug!(
                        inner_turn = turn_ctx.inner_turn,
                        reasoning_len = completion
                            .reasoning_content
                            .as_ref()
                            .map(|s| s.len())
                            .unwrap_or(0),
                        stop_reason = ?completion.stop_reason,
                        "inner_turn_continue_reasoning_only"
                    );
                    match turn_ctx.increment_inner() {
                        Ok(()) => continue,
                        Err(e) => return Ok(e),
                    }
                }
                tracing::debug!(
                    inner_turn = turn_ctx.inner_turn,
                    content_len = completion.content.as_ref().map(|s| s.len()).unwrap_or(0),
                    reasoning_len = completion
                        .reasoning_content
                        .as_ref()
                        .map(|s| s.len())
                        .unwrap_or(0),
                    stop_reason = ?completion.stop_reason,
                    "inner_turn_terminal_no_tools"
                );
                return Ok(TerminalReason::Completed);
            }

            // Live LLM: assistant + tool results already persisted in call_llm_and_execute.
            // Offline mock has no streaming executor — persist assistant here.
            if self.llm.is_none() {
                let assistant = assistant_from_completion(&completion);
                self.persist_message_alloc(&assistant)?;
                self.messages.push(assistant);
            }

            // Tool results are in self.messages from execute_stream_results (live LLM only).

            match turn_ctx.increment_inner() {
                Ok(()) => {}
                Err(TerminalReason::MaxReactLoops(n)) => {
                    return Ok(TerminalReason::MaxReactLoops(n))
                }
                Err(other) => return Ok(other),
            }
        }
    }

    // ── LLM call + streaming tool execution (unified path) ────

    fn append_interrupt_message(&mut self, _tool_use: bool) -> Result<(), AgentError> {
        // Suppressed — "[Request interrupted by user]" bubble is pointless noise.
        // Drain request (max_tokens=1, stream=false) runs in background to
        // keep session token counts accurate (see drain_usage_background).
        Ok(())
    }

    async fn call_llm_and_execute(
        &mut self,
        messages: &[LlmChatMessage],
        tools: &[(String, String, serde_json::Value)],
        turn_ctx: &TurnContext,
        event_tx: Option<&mpsc::UnboundedSender<Event>>,
        persist_tool_messages: bool,
    ) -> Result<LlmCallOutcome, AgentError> {
        let max_concurrent = self.shared.settings.agent.max_tool_concurrency;
        let session_id = self.shared.session.id.clone();
        let model = self.shared.settings.model.model.clone();

        if self.llm.is_some() {
            tracing::debug!(
                session_id = %session_id,
                inner_turn = turn_ctx.inner_turn,
                message_count = messages.len(),
                tool_schema_count = tools.len(),
                "llm_request_start"
            );
            self.audit_log(LogEvent::LlmRequest {
                session_id: session_id.clone(),
                model: model.clone(),
                streaming: true,
            });
            let tx = event_tx.cloned();
            let audit = self.shared.audit.clone();
            let cancel_flag = self.shared.abort_controller.cancel_flag();
            let initial_abort = map_abort_signal(self.abort_reason());
            let (abort_tool_tx, abort_tool_rx) = novel_tools::abort_channel();
            let _ = abort_tool_tx.send(initial_abort);
            let bridge = tokio::spawn(run_abort_bridge(
                Arc::clone(&self.shared.abort_controller),
                abort_tool_tx,
            ));

            let ctx = self.tool_context();
            let dispatch_arc = Arc::new(Mutex::new(StreamingToolDispatch::new(
                Arc::clone(&self.shared.registry),
                ctx.clone(),
                max_concurrent,
                abort_tool_rx,
            )));

            let stream_done = Arc::new(AtomicBool::new(false));
            let stream_done_poll = Arc::clone(&stream_done);
            let dispatch_poll = Arc::clone(&dispatch_arc);
            let event_tx_poll = event_tx.cloned();
            let poll_handle = tokio::spawn(async move {
                while !stream_done_poll.load(Ordering::SeqCst) {
                    tokio::time::sleep(Duration::from_millis(40)).await;
                    if let Ok(mut d) = dispatch_poll.lock() {
                        d.poll_ui_results(event_tx_poll.as_ref());
                    }
                }
            });

            let dispatch_cb = Arc::clone(&dispatch_arc);
            let registry_cb = Arc::clone(&self.shared.registry);
            let ctx_cb = ctx.clone();
            let event_tx_cb = event_tx.cloned();
            let on_tool = move |tc: LlmToolCall| {
                if let Ok(mut d) = dispatch_cb.lock() {
                    d.handle_ready(&registry_cb, &ctx_cb, event_tx_cb.as_ref(), tc, false);
                }
            };

            let client = self.llm.as_mut().expect("llm checked");
            let stream_result = client
                .create_stream(
                    messages,
                    tools,
                    self.shared.settings.model.max_output_tokens,
                    move |ev: StreamEvent| {
                        if let Some(ref tx) = tx {
                            match ev {
                                StreamEvent::ContentBlockDelta { delta, kind, .. } => {
                                    let _ = tx.send(Event::ContentBlockDelta {
                                        message_id: String::new(),
                                        index: 0,
                                        delta,
                                        kind,
                                    });
                                }
                                StreamEvent::ToolUseStarted {
                                    tool_call_id, name, ..
                                } => {
                                    let _ = tx.send(Event::ToolUseStarted { tool_call_id, name });
                                }
                                StreamEvent::ToolInputDelta {
                                    tool_call_id,
                                    delta,
                                } => {
                                    let _ = tx.send(Event::ToolInputDelta {
                                        tool_call_id,
                                        delta,
                                    });
                                }
                                StreamEvent::MessageStop { .. } => {}
                                StreamEvent::StreamError { message, .. } => {
                                    tracing::warn!(error = %message, "llm_stream_error");
                                    if let Some(ref a) = audit {
                                        let _ = a.log(&LogEvent::Error {
                                            message: message.clone(),
                                            recoverable: true,
                                        });
                                    }
                                    let _ = tx.send(Event::Error {
                                        message,
                                        recoverable: true,
                                    });
                                }
                            }
                        }
                    },
                    Some(on_tool),
                    Some(cancel_flag),
                )
                .await;

            stream_done.store(true, Ordering::SeqCst);
            let _ = poll_handle.await;
            bridge.abort();

            let (completion, stream_aborted, bg_usage) = match stream_result {
                Err(e) => {
                    tracing::warn!(error = %e, "llm_request_failed");
                    self.audit_error(e.to_string(), true);
                    return Err(AgentError::Llm(e));
                }
                Ok(StreamOutcome::Complete(c)) => (c, false, None),
                Ok(StreamOutcome::Cancelled {
                    partial,
                    background_usage,
                }) => (partial, true, Some(background_usage)),
            };

            if stream_aborted || self.interrupt_requested() {
                if let Ok(mut dispatch) = dispatch_arc.lock() {
                    dispatch.discard();
                }
                let shared = self.shared.clone();
                let llm_snap = read_session_llm(&shared);
                let llm_messages = messages.to_vec();
                let tool_schemas = tools.to_vec();
                let mut sink = MainSessionSink { engine: self };
                let usage = finalize_stream_cancel(FinalizeStreamCancelParams {
                    sink: &mut sink,
                    partial: completion,
                    llm_messages,
                    tool_schemas,
                    background_usage: bg_usage,
                    llm_snap,
                    shared,
                    event_tx,
                })
                .await?;
                self.last_turn_usage = usage.clone();
                if let Some(u) = usage.as_ref() {
                    tracing::debug!(
                        cache_hit = u.cache_hit_tokens,
                        cache_miss = u.cache_miss_tokens,
                        completion = u.completion_tokens,
                        "token_usage_recorded_stream_abort"
                    );
                }
                self.append_interrupt_message(true)?;
                return Ok(LlmCallOutcome::Aborted(TerminalReason::AbortedStreaming));
            }

            tracing::debug!(
                session_id = %session_id,
                tool_call_count = completion.tool_calls.len(),
                has_usage = completion.usage.is_some(),
                stream_aborted,
                "llm_request_complete"
            );

            self.record_usage(&completion, event_tx);

            if let Some(tx) = event_tx {
                let _ = tx.send(Event::AssistantSegmentComplete {
                    segment_index: turn_ctx.inner_turn,
                    fork_run_id: None,
                });
            }

            if completion.tool_calls.is_empty() {
                if let Ok(mut dispatch) = dispatch_arc.lock() {
                    dispatch.drain_pending_specs(self);
                }
                return Ok(LlmCallOutcome::Continue(completion));
            }

            let (executed_specs, skip_result_events, denied_specs, mut executor) = {
                let mut dispatch = dispatch_arc.lock().map_err(|_| {
                    AgentError::Validation("streaming tool dispatch lock poisoned".into())
                })?;
                for tc in &completion.tool_calls {
                    if !dispatch.handled_ids.contains(&tc.id) {
                        dispatch.handle_ready(
                            &self.shared.registry,
                            &ctx,
                            event_tx,
                            tc.clone(),
                            true,
                        );
                    } else if let Some(tx) = event_tx {
                        let input = parse_tool_call_input(&tc.arguments, &tc.id, &tc.name);
                        let needs_approval = self.pending_tools.contains_key(&tc.id)
                            || dispatch.pending_specs.contains_key(&tc.id);
                        let _ = tx.send(Event::ToolCallRequest {
                            tool_call_id: tc.id.clone(),
                            name: tc.name.clone(),
                            input,
                            needs_approval,
                        });
                    }
                }
                for (_, spec) in dispatch.pending_specs.drain() {
                    self.pending_tools.insert(spec.id.clone(), spec);
                }
                self.has_interruptible_tool_in_progress =
                    dispatch.executor_mut().has_interruptible_tool_in_progress();
                dispatch.poll_ui_results(event_tx);
                let executor = dispatch.take_executor();
                let executed_specs = std::mem::take(&mut dispatch.executed_specs);
                let skip_result_events = std::mem::take(&mut dispatch.ui_result_emitted);
                let denied_specs = std::mem::take(&mut dispatch.denied_specs);
                (executed_specs, skip_result_events, denied_specs, executor)
            };

            let mut results = executor.get_remaining_results().await;
            for (id, (_, reason)) in denied_specs {
                results.push((id, Err(novel_tools::ToolError::PermissionDenied(reason))));
            }
            self.has_interruptible_tool_in_progress = false;
            for spec in &executed_specs {
                self.pending_tools.remove(&spec.id);
            }

            let assistant = assistant_from_completion(&completion);
            self.persist_message_alloc(&assistant)?;
            self.messages.push(assistant);

            let tool_call_order: Vec<String> = completion
                .tool_calls
                .iter()
                .map(|tc| tc.id.clone())
                .collect();

            if self.interrupt_requested() {
                let _ = self
                    .execute_stream_results(
                        results,
                        &executed_specs,
                        &tool_call_order,
                        &skip_result_events,
                        event_tx,
                        persist_tool_messages,
                    )
                    .await?;
                self.append_interrupt_message(true)?;
                return Ok(LlmCallOutcome::Aborted(TerminalReason::AbortedTools));
            }

            if self
                .execute_stream_results(
                    results,
                    &executed_specs,
                    &tool_call_order,
                    &skip_result_events,
                    event_tx,
                    persist_tool_messages,
                )
                .await?
            {
                return Ok(LlmCallOutcome::Continue(completion));
            }

            Ok(LlmCallOutcome::Continue(completion))
        } else {
            let completion = ChatClient::offline_complete(messages);
            if let Some(tx) = event_tx {
                if let Some(content) = &completion.content {
                    let _ = tx.send(Event::ContentBlockDelta {
                        message_id: String::new(),
                        index: 0,
                        delta: content.clone(),
                        kind: ContentBlockKind::Text,
                    });
                }
                let _ = tx.send(Event::AssistantSegmentComplete {
                    segment_index: turn_ctx.inner_turn,
                    fork_run_id: None,
                });
            }
            Ok(LlmCallOutcome::Continue(completion))
        }
    }
} // end first impl AgentEngine block

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
    novel_tools::format_tool_result_for_llm(spec_ref, result)
}

impl AgentEngine {
    /// Returns true if turn should pause (e.g. AskUserQuestion).
    #[allow(clippy::too_many_arguments)]
    async fn execute_stream_results(
        &mut self,
        results: Vec<(
            String,
            Result<novel_tools::ToolOutput, novel_tools::ToolError>,
        )>,
        executed_specs: &[ToolCallSpec],
        tool_call_order: &[String],
        _skip_result_events: &HashSet<String>,
        event_tx: Option<&mpsc::UnboundedSender<Event>>,
        persist_tool_messages: bool,
    ) -> Result<bool, AgentError> {
        let spec_by_id: std::collections::HashMap<&str, &ToolCallSpec> =
            executed_specs.iter().map(|s| (s.id.as_str(), s)).collect();

        // Prefer a single result per tool_call_id. NeedsUserInput (AskUserQuestion) wins
        // over Ok when the streaming executor mis-reports after concurrent completion.
        let mut by_id: HashMap<String, Result<novel_tools::ToolOutput, novel_tools::ToolError>> =
            HashMap::new();
        for (id, result) in results {
            match by_id.get(&id) {
                None => {
                    by_id.insert(id, result);
                }
                Some(existing) => {
                    let incoming_needs_input =
                        matches!(&result, Err(novel_tools::ToolError::NeedsUserInput { .. }));
                    let existing_needs_input =
                        matches!(existing, Err(novel_tools::ToolError::NeedsUserInput { .. }));
                    if incoming_needs_input {
                        by_id.insert(id, result);
                    } else if existing_needs_input {
                        // Keep AskUserQuestion pause signal.
                    } else if result.is_ok() && existing.is_err() {
                        by_id.insert(id, result);
                    }
                }
            }
        }

        let mut ordered_ids: Vec<String> = tool_call_order
            .iter()
            .filter(|id| by_id.contains_key(*id))
            .cloned()
            .collect();
        for id in by_id.keys() {
            if !ordered_ids.iter().any(|o| o == id) {
                ordered_ids.push(id.clone());
            }
        }

        let mut pause_for_question = false;
        for id in ordered_ids {
            let result = match by_id.remove(&id) {
                Some(r) => r,
                None => continue,
            };
            let spec = spec_by_id.get(id.as_str()).copied();
            match result {
                Ok(out) => {
                    let success = !out.is_error;
                    let formatted = format_tool(spec, Ok(out));
                    let content = formatted.content;
                    let hook_task = if self.shared.settings.hooks.post_tool_use.is_empty() {
                        None
                    } else {
                        spec.and_then(|s| {
                            crate::hooks::knowledge_auditor_hook_task(
                                &self.shared.settings.hooks,
                                &s.name,
                                Some(&s.input),
                                &formatted.hook_preview,
                            )
                        })
                    };

                    // Track last chapter written (for progress display in system prompt)
                    if let Some(s) = spec {
                        if let Some(path) = novel_tools::optional_file_path(&s.input) {
                            if novel_tools::normalize_rel_path(&path).contains("chapters/") {
                                self.last_chapter_written =
                                    Some(novel_tools::normalize_chapter_progress_path(&path));
                            }
                        }
                    }

                    if let Some(tx) = event_tx {
                        let _ = tx.send(Event::ToolCallResult {
                            tool_call_id: id.clone(),
                            content: content.clone(),
                        });
                    }
                    let tool_msg = tool_result_message(&id, &content);
                    if persist_tool_messages {
                        self.persist_message_alloc(&tool_msg)?;
                    }
                    if let Some(s) = spec {
                        tracing::debug!(
                            tool_call_id = %id,
                            tool_name = %s.name,
                            success,
                            "tool_executed"
                        );
                        self.audit_log(LogEvent::ToolExecuted {
                            session_id: self.shared.session.id.clone(),
                            tool_name: s.name.clone(),
                            success,
                        });
                    }
                    self.messages.push(tool_msg);

                    if let Some(s) = spec {
                        // Track invoked skills for post-compaction re-injection
                        if s.name == "InvokeSkill" {
                            if let Some(skill_id) = s
                                .input
                                .get("skill_id")
                                .or_else(|| s.input.get("skillId"))
                                .and_then(|v| v.as_str())
                            {
                                if !self.invoked_skill_ids.iter().any(|id| id == skill_id) {
                                    self.invoked_skill_ids.push(skill_id.to_string());
                                    let _ = self.shared.session.db.set_invoked_skill_ids(
                                        &self.shared.session.id,
                                        &self.invoked_skill_ids,
                                    );
                                }
                            }
                        }
                        if s.name == "Read" && success {
                            if let Some(path) = novel_tools::optional_file_path(&s.input) {
                                if let Some((_, canonical)) = parse_skill_reference_path(
                                    &self.shared.session.project_root,
                                    &self.shared.agent_skills_dir,
                                    &path,
                                ) {
                                    if !self
                                        .read_skill_reference_paths
                                        .iter()
                                        .any(|p| p == &canonical)
                                    {
                                        self.read_skill_reference_paths.push(canonical);
                                        let _ =
                                            self.shared.session.db.set_read_skill_reference_paths(
                                                &self.shared.session.id,
                                                &self.read_skill_reference_paths,
                                            );
                                    }
                                }
                            }
                        }
                        // Opt-in PostToolUse hooks (settings.json); default config is empty.
                        // Matching is now handled by hook_config.matcher, not hardcoded tool names.
                        if let Some(task) = hook_task {
                            if let Ok(mut guard) = self.shared.subagent_queue.lock() {
                                guard.push(PendingSubagentWork {
                                    agent_type: "KnowledgeAuditor".into(),
                                    task,
                                    parent_tool_call_id: None,
                                });
                            }
                        }
                    }
                }
                Err(novel_tools::ToolError::NeedsUserInput { payload }) => {
                    tracing::debug!(tool_call_id = %id, "tool_needs_user_input");
                    self.pending_user_question = Some(id.clone());
                    if let Some(s) = spec {
                        self.audit_log(LogEvent::ToolExecuted {
                            session_id: self.shared.session.id.clone(),
                            tool_name: s.name.clone(),
                            success: true,
                        });
                    }
                    if let Some(tx) = event_tx {
                        let _ = tx.send(Event::AskUserQuestion {
                            tool_call_id: id.clone(),
                            payload: payload.clone(),
                        });
                    }
                    let tool_msg = tool_result_message(&id, novel_tools::NEEDS_USER_INPUT_STUB);
                    if persist_tool_messages {
                        self.persist_message_alloc(&tool_msg)?;
                    }
                    self.messages.push(tool_msg);
                    pause_for_question = true;
                }
                Err(e) => {
                    if let Some(s) = spec {
                        tracing::warn!(
                            tool_call_id = %id,
                            tool_name = %s.name,
                            error = %e,
                            "tool_executed_failed"
                        );
                        self.audit_log(LogEvent::ToolExecuted {
                            session_id: self.shared.session.id.clone(),
                            tool_name: s.name.clone(),
                            success: false,
                        });
                    }
                    let msg = format_tool(spec, Err(e)).content;
                    if let Some(tx) = event_tx {
                        let _ = tx.send(Event::ToolCallResult {
                            tool_call_id: id.clone(),
                            content: msg.clone(),
                        });
                    }
                    let tool_msg = tool_result_message(&id, &msg);
                    if persist_tool_messages {
                        self.persist_message_alloc(&tool_msg)?;
                    }
                    self.messages.push(tool_msg);
                }
            }
        }
        Ok(pause_for_question)
    }

    // ── approve_tool / deny_tool ───────────────────────────────

    pub async fn approve_tool(
        &mut self,
        tool_call_id: &str,
        event_tx: Option<&mpsc::UnboundedSender<Event>>,
    ) -> Result<(), AgentError> {
        let spec = self.pending_tools.remove(tool_call_id).ok_or_else(|| {
            tracing::warn!(%tool_call_id, "approve_tool: unknown tool approval");
            AgentError::Validation("unknown tool approval".into())
        })?;
        tracing::debug!(
            %tool_call_id,
            tool_name = %spec.name,
            "approve_tool"
        );
        let ctx = self.tool_context();
        let executor = ToolExecutor::new(Arc::clone(&self.shared.registry));
        let result = executor.execute_one_user_approved(&spec, &ctx).await;
        let success = matches!(&result, Ok(out) if !out.is_error);
        let content = format_tool(Some(&spec), result).content;
        self.audit_log(LogEvent::ToolExecuted {
            session_id: self.shared.session.id.clone(),
            tool_name: spec.name.clone(),
            success,
        });
        if let Some(tx) = event_tx {
            let _ = tx.send(Event::ToolCallResult {
                tool_call_id: tool_call_id.to_string(),
                content: content.clone(),
            });
        }
        let tool_msg = tool_result_message(tool_call_id, &content);
        self.messages.push(tool_msg.clone());
        self.persist_message_alloc(&tool_msg)?;
        if event_tx.is_some()
            && self.pending_tools.is_empty()
            && self.pending_user_question.is_none()
        {
            self.continue_turn_loop(event_tx).await?;
        } else if event_tx.is_some() && self.pending_user_question.is_some() {
            tracing::debug!(
                %tool_call_id,
                "tool_resolved_turn_still_paused_for_question"
            );
        }
        Ok(())
    }

    pub async fn deny_tool(
        &mut self,
        tool_call_id: &str,
        reason: Option<String>,
        event_tx: Option<&mpsc::UnboundedSender<Event>>,
    ) -> Result<(), AgentError> {
        let spec = self.pending_tools.remove(tool_call_id).ok_or_else(|| {
            tracing::warn!(%tool_call_id, "deny_tool: unknown tool approval");
            AgentError::Validation("unknown tool approval".into())
        })?;
        tracing::debug!(
            %tool_call_id,
            tool_name = %spec.name,
            "deny_tool"
        );
        self.audit_log(LogEvent::ToolExecuted {
            session_id: self.shared.session.id.clone(),
            tool_name: spec.name,
            success: false,
        });
        let content = reason.unwrap_or_else(|| "denied by user".into());
        if let Some(tx) = event_tx {
            let _ = tx.send(Event::ToolCallResult {
                tool_call_id: tool_call_id.to_string(),
                content: content.clone(),
            });
        }
        let msg = tool_result_message(tool_call_id, &content);
        self.persist_message_alloc(&msg)?;
        self.messages.push(msg);
        if event_tx.is_some()
            && self.pending_tools.is_empty()
            && self.pending_user_question.is_none()
        {
            self.continue_turn_loop(event_tx).await?;
        } else if event_tx.is_some() && self.pending_user_question.is_some() {
            tracing::debug!(
                %tool_call_id,
                "tool_denied_turn_still_paused_for_question"
            );
        }
        Ok(())
    }

    /// Submit user answers for a pending AskUserQuestion and continue the turn.
    pub async fn answer_question(
        &mut self,
        tool_call_id: &str,
        answers: serde_json::Value,
        event_tx: Option<&mpsc::UnboundedSender<Event>>,
    ) -> Result<TerminalReason, AgentError> {
        let pending = self
            .pending_user_question
            .as_ref()
            .ok_or_else(|| AgentError::Validation("no pending question".into()))?;
        if pending != tool_call_id {
            tracing::warn!(
                expected = %pending,
                %tool_call_id,
                "answer_question: tool_call_id mismatch"
            );
            return Err(AgentError::Validation("tool_call_id mismatch".into()));
        }
        tracing::debug!(%tool_call_id, "answer_question");
        self.pending_user_question = None;

        let content =
            serde_json::to_string(&answers).map_err(|e| AgentError::Validation(e.to_string()))?;

        if let Some(msg) = self
            .messages
            .iter_mut()
            .rev()
            .find(|m| m.role == "tool" && m.tool_call_id.as_deref() == Some(tool_call_id))
        {
            msg.content = content.clone();
        } else {
            let tool_msg = tool_result_message(tool_call_id, &content);
            self.persist_message_alloc(&tool_msg)?;
            self.messages.push(tool_msg);
        }

        if let Some(tx) = event_tx {
            let _ = tx.send(Event::ToolCallResult {
                tool_call_id: tool_call_id.to_string(),
                content,
            });
        }

        self.continue_turn_loop(event_tx).await
    }

    async fn continue_turn_loop(
        &mut self,
        event_tx: Option<&mpsc::UnboundedSender<Event>>,
    ) -> Result<TerminalReason, AgentError> {
        tracing::debug!(
            session_id = %self.shared.session.id,
            turn_number = self.turn_number,
            "turn_continue"
        );
        self.init_llm();
        self.init_turn_message_seq_from_db()?;
        let mut turn_ctx = TurnContext::new(self.turn_number, MAIN_MAX_INNER_TURNS);
        let resume_inner = self.resume_inner_turn_from_messages();
        turn_ctx.inner_turn = resume_inner;
        turn_ctx.inner_turn_at_start = resume_inner;
        tracing::debug!(
            resume_inner_turn = resume_inner,
            inner_turn_at_start = resume_inner,
            assistant_count = resume_inner,
            message_count = self.messages.len(),
            "turn_continue_resume_inner_turn"
        );
        let reason = self.run_inner_turn_loop(&mut turn_ctx, event_tx).await?;
        let (hit, miss, comp, _ctx) = self.session_token_summary();
        let (th, tm, tc) = self
            .last_turn_usage
            .as_ref()
            .map(|u| (u.cache_hit_tokens, u.cache_miss_tokens, u.completion_tokens))
            .unwrap_or((0, 0, 0));
        tracing::debug!(
            session_id = %self.shared.session.id,
            turn_number = self.turn_number,
            ?reason,
            resume_inner_turn = resume_inner,
            "turn_continue_complete"
        );
        self.audit_log(LogEvent::TurnCompleted {
            session_id: self.shared.session.id.clone(),
            turn_number: self.turn_number,
            cache_hit_tokens: hit,
            cache_miss_tokens: miss,
            completion_tokens: comp,
        });
        let turn_paused = self.pending_user_question.is_some() || !self.pending_tools.is_empty();
        if let Some(tx) = event_tx {
            if reason.is_aborted() {
                self.audit_error("用户已中断", true);
            }
            if turn_paused {
                tracing::debug!(
                    pending_question = self.pending_user_question.is_some(),
                    pending_tool_count = self.pending_tools.len(),
                    "turn_continue_paused"
                );
            } else {
                let _ = tx.send(Event::TurnComplete {
                    turn_number: self.turn_number,
                    cache_hit_tokens: hit,
                    cache_miss_tokens: miss,
                    completion_tokens: comp,
                    turn_hit_tokens: th,
                    turn_miss_tokens: tm,
                    turn_comp_tokens: tc,
                    was_interrupted: reason.is_aborted(),
                });
            }
        }
        Ok(reason)
    }

    // ── Compaction ─────────────────────────────────────────────

    /// Check if compaction is needed based on real context token count from the last API call.
    pub(crate) fn compaction_needed(&self) -> bool {
        if self.last_context_tokens == 0 {
            return false;
        }
        let threshold = self.shared.context_manager.threshold();
        let window = self.shared.context_manager.window_size();
        self.last_context_tokens as f32 / window as f32 >= threshold
    }

    async fn generate_summary_text(
        &mut self,
        summarize_to: usize,
        to_summarize: &[novel_compaction::CompactionMessage],
        max_chars: usize,
        max_output_tokens: u32,
    ) -> String {
        use novel_compaction::{
            build_summary_trailing_user_prompt, rule_based_summary, truncate_summary,
        };

        let fallback = || rule_based_summary(to_summarize, max_chars);

        if summarize_to <= 1 {
            return fallback();
        }

        if self.interrupt_requested() {
            return fallback();
        }

        let Some(llm) = self.llm.as_mut() else {
            return fallback();
        };

        let prefix_end = summarize_to.min(self.messages.len());
        let mut llm_msgs = to_llm_messages(&self.messages[..prefix_end]);
        llm_msgs.push(LlmChatMessage {
            role: "user".into(),
            content: build_summary_trailing_user_prompt(),
            tool_call_id: None,
            tool_calls: None,
            reasoning_content: None,
        });

        let cancel = Some(self.shared.abort_controller.cancel_flag());
        match llm
            .complete_via_stream(&llm_msgs, &[], max_output_tokens, cancel)
            .await
        {
            Ok(r) => r
                .content
                .filter(|c| !c.contains(ERROR_MESSAGE_USER_ABORT) && !c.trim().is_empty())
                .map(|c| truncate_summary(&c, max_chars))
                .unwrap_or_else(fallback),
            Err(LlmError::Cancelled) => fallback(),
            Err(_) => fallback(),
        }
    }

    /// Compact context and replace session messages. Main agent only.
    const MAX_CONSECUTIVE_COMPACTION_FAILURES: u32 = 3;

    async fn compact_and_sync(
        &mut self,
        event_tx: Option<&mpsc::UnboundedSender<Event>>,
    ) -> Result<(), AgentError> {
        // Circuit breaker: skip after N consecutive failures
        if self.compaction_fail_count >= Self::MAX_CONSECUTIVE_COMPACTION_FAILURES {
            tracing::warn!(
                fail_count = self.compaction_fail_count,
                "compaction_skipped_circuit_breaker"
            );
            return Ok(());
        }

        let compaction_lock = Arc::clone(&self.shared.compaction_lock);
        let _guard = compaction_lock.lock().await;

        if !self.compaction_needed() {
            return Ok(());
        }

        if self.interrupt_requested() {
            return Ok(());
        }

        let attempt = self.compaction_fail_count + 1;
        let emit = |tx: Option<&mpsc::UnboundedSender<Event>>, action: CompactionAction| {
            if let Some(tx) = tx {
                let _ = tx.send(Event::CompactionProgress { attempt, action });
            }
        };
        emit(event_tx, CompactionAction::Started);
        tracing::info!(tokens_before = self.last_context_tokens, "compaction_start");
        tracing::debug!(
            session_id = %self.shared.session.id,
            message_count = self.messages.len(),
            attempt,
            "compaction_start_detail"
        );

        use novel_compaction::{apply_level4_compaction, partition_messages};

        let retain = self.shared.context_manager.retain_policy().clone();
        let compacted = chat_slice_to_compaction(&self.messages);
        let partition = partition_messages(&compacted, retain.recent_react_turns);
        let to_summarize = if partition.summarize_to > partition.summarize_from {
            &compacted[partition.summarize_from..partition.summarize_to]
        } else {
            &[]
        };

        emit(event_tx, CompactionAction::GeneratingSummary);
        let summary_text = self
            .generate_summary_text(
                partition.summarize_to,
                to_summarize,
                retain.summary_max_chars,
                retain.summary_max_output_tokens,
            )
            .await;

        emit(event_tx, CompactionAction::RebuildingSession);

        let epoch = self
            .shared
            .session
            .db
            .increment_compaction_count(&self.shared.session.id)
            .map_err(AgentError::from)?;
        self.shared
            .session
            .db
            .archive_session_messages(&self.shared.session.id, epoch)
            .map_err(AgentError::from)?;

        self.refresh_system_dynamic_sections()?;

        let skill_ids = filter_loadable_skill_ids(
            &self.shared.session.project_root,
            &self.shared.agent_skills_dir,
            &self.invoked_skill_ids,
        );
        let ref_paths = filter_loadable_reference_paths(
            &self.shared.session.project_root,
            &self.shared.agent_skills_dir,
            &self.read_skill_reference_paths,
            &skill_ids,
        );
        let skill_bodies = format_activated_skill_block(
            &self.shared.session.project_root,
            &self.shared.agent_skills_dir,
            &skill_ids,
            &ref_paths,
        );

        let system = chat_to_compaction(
            self.messages
                .first()
                .ok_or_else(|| AgentError::Validation("no system message".into()))?,
        );
        let to_retain = if partition.retain_from < self.messages.len() {
            chat_slice_to_compaction(&self.messages[partition.retain_from..])
        } else {
            vec![]
        };

        let window = self.shared.context_manager.window_size();
        let root = self.shared.session.project_root.clone();

        let final_msgs = apply_level4_compaction(
            system,
            &summary_text,
            to_retain,
            &skill_bodies,
            &skill_ids,
            &retain,
            &root,
            window,
        )?;

        self.invoked_skill_ids = skill_ids.clone();
        let _ = self
            .shared
            .session
            .db
            .set_invoked_skill_ids(&self.shared.session.id, &skill_ids);
        self.read_skill_reference_paths = ref_paths.clone();
        let _ = self
            .shared
            .session
            .db
            .set_read_skill_reference_paths(&self.shared.session.id, &ref_paths);

        let tokens_before = self.last_context_tokens;
        self.audit_log(LogEvent::CompactionTriggered {
            session_id: self.shared.session.id.clone(),
            level: "level4".into(),
            tokens_before,
        });
        self.messages = compaction_slice_to_chat(&final_msgs);
        self.sync_messages_to_db()?;
        self.shared.clear_read_file_cache();
        tracing::debug!(
            session_id = %self.shared.session.id,
            "read_file_cache_cleared_after_compaction"
        );
        self.last_context_tokens = 0;

        // Success: reset fail counter
        self.compaction_fail_count = 0;
        tracing::info!(
            tokens_before,
            messages = self.messages.len(),
            "compaction_done"
        );
        tracing::debug!(
            session_id = %self.shared.session.id,
            tokens_after = self.last_context_tokens,
            "compaction_done_detail"
        );
        emit(
            event_tx,
            CompactionAction::Done {
                tokens_before,
                tokens_after: self.last_context_tokens,
            },
        );

        Ok(())
    }

    /// Wraps compact_and_sync with circuit-breaker counting.
    pub(crate) async fn compact_with_events(
        &mut self,
        event_tx: Option<&mpsc::UnboundedSender<Event>>,
    ) {
        match self.compact_and_sync(event_tx).await {
            Ok(()) => {}
            Err(e) => {
                self.compaction_fail_count += 1;
                let reason = format!("{e}");
                tracing::warn!(
                    error = %e,
                    fail_count = self.compaction_fail_count,
                    "compaction_failed"
                );
                self.audit_error(reason.clone(), true);
                if let Some(tx) = event_tx {
                    let _ = tx.send(Event::CompactionProgress {
                        attempt: self.compaction_fail_count,
                        action: CompactionAction::Failed { reason },
                    });
                }
            }
        }
    }

    fn sync_messages_to_db(&self) -> Result<(), AgentError> {
        let rows = self.build_message_rows();
        let refs: Vec<(i32, i32, &str, &serde_json::Value)> = rows
            .iter()
            .map(|(t, s, r, v)| (*t, *s, r.as_str(), v))
            .collect();
        if let Err(e) = self
            .shared
            .session
            .db
            .replace_session_messages(&self.shared.session.id, &refs)
        {
            tracing::error!(
                error = %e,
                row_count = refs.len(),
                "sync_messages_to_db_failed"
            );
            return Err(AgentError::State(e));
        }
        tracing::debug!(
            session_id = %self.shared.session.id,
            row_count = refs.len(),
            "sync_messages_to_db"
        );
        Ok(())
    }

    fn build_message_rows(&self) -> Vec<(i32, i32, String, serde_json::Value)> {
        let mut rows = Vec::with_capacity(self.messages.len());
        let mut turn = 0i32;
        let mut seq_in_turn = 0i32;
        for msg in self.messages.iter() {
            let (t, seq) = if msg.role == "system" {
                turn = 0;
                seq_in_turn = 0;
                (0, 0)
            } else if msg.content.starts_with("[上下文刷新]") {
                (0, 1)
            } else if Self::is_sub_agent_report(msg) {
                seq_in_turn += 1;
                (turn, seq_in_turn)
            } else if msg.role == "user" {
                turn += 1;
                seq_in_turn = 0;
                (turn, MSG_SEQ_USER)
            } else {
                seq_in_turn += 1;
                (turn, seq_in_turn)
            };
            rows.push((t, seq, msg.role.clone(), chat_to_json(msg)));
        }
        rows
    }

    // ── Persistence helpers ────────────────────────────────────

    fn alloc_turn_message_seq(&mut self) -> i32 {
        self.turn_message_seq += 1;
        self.turn_message_seq
    }

    fn persist_message_at_seq(
        &mut self,
        msg: &ChatMessage,
        sequence: i32,
    ) -> Result<(), AgentError> {
        if sequence > self.turn_message_seq {
            self.turn_message_seq = sequence;
        }
        let content_len = msg.content.len();
        tracing::debug!(
            session_id = %self.shared.session.id,
            turn_number = self.turn_number,
            role = %msg.role,
            sequence,
            content_len,
            "persist_message"
        );
        if let Err(e) = self.shared.session.db.insert_message(
            &self.shared.session.id,
            self.turn_number as i32,
            sequence,
            &msg.role,
            &chat_to_json(msg),
            None,
        ) {
            tracing::error!(
                error = %e,
                role = %msg.role,
                sequence,
                turn_number = self.turn_number,
                "persist_message_failed"
            );
            return Err(AgentError::State(e));
        }
        Ok(())
    }

    fn init_turn_message_seq_from_db(&mut self) -> Result<(), AgentError> {
        let max = self
            .shared
            .session
            .db
            .max_message_sequence_for_turn(&self.shared.session.id, self.turn_number as i32)
            .map_err(AgentError::State)?;
        self.turn_message_seq = max;
        Ok(())
    }

    pub(crate) fn persist_message_alloc(&mut self, msg: &ChatMessage) -> Result<(), AgentError> {
        self.persist_message_alloc_ex(msg, None).map(|_| ())
    }

    /// Persist to parent session `messages`; returns row id. `extra` merges UI metadata (e.g. fork_run_id).
    fn persist_message_alloc_ex(
        &mut self,
        msg: &ChatMessage,
        extra: Option<&serde_json::Value>,
    ) -> Result<String, AgentError> {
        let sequence = self.alloc_turn_message_seq();
        let mut json = chat_to_json(msg);
        if let Some(extra) = extra {
            if let (Some(obj), Some(extra_obj)) = (json.as_object_mut(), extra.as_object()) {
                for (k, v) in extra_obj {
                    obj.insert(k.clone(), v.clone());
                }
            }
        }
        let content_len = msg.content.len();
        tracing::debug!(
            session_id = %self.shared.session.id,
            turn_number = self.turn_number,
            role = %msg.role,
            sequence,
            content_len,
            "persist_message"
        );
        self.shared
            .session
            .db
            .insert_message(
                &self.shared.session.id,
                self.turn_number as i32,
                sequence,
                &msg.role,
                &json,
                None,
            )
            .map_err(|e| {
                tracing::error!(
                    error = %e,
                    role = %msg.role,
                    sequence,
                    turn_number = self.turn_number,
                    "persist_message_failed"
                );
                AgentError::State(e)
            })
    }

    fn record_usage(
        &mut self,
        completion: &LlmCompletion,
        event_tx: Option<&mpsc::UnboundedSender<Event>>,
    ) {
        self.sync_session_llm_from_llm();
        if let Some(u) = &completion.usage {
            self.last_turn_usage = Some(u.clone());
            let snap = read_session_llm(&self.shared);
            apply_session_usage(&self.shared, u, &snap, event_tx);
            tracing::debug!(
                cache_hit = u.cache_hit_tokens,
                cache_miss = u.cache_miss_tokens,
                completion = u.completion_tokens,
                "token_usage_recorded"
            );
        } else {
            self.last_turn_usage = None;
            let _ = self
                .shared
                .session
                .db
                .touch_last_active_at(&self.shared.session.id);
        }
    }

    pub fn session_token_summary(&self) -> (i64, i64, i64, i64) {
        self.shared
            .session
            .db
            .get_session(&self.shared.session.id)
            .ok()
            .flatten()
            .map(|s| {
                (
                    s.cache_hit_tokens,
                    s.cache_miss_tokens,
                    s.completion_tokens,
                    s.context_tokens,
                )
            })
            .unwrap_or((0, 0, 0, 0))
    }

    fn main_tool_names(&self) -> Vec<String> {
        self.shared.registry.names()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::EngineConfig;
    use tempfile::TempDir;

    fn test_config(tmp: &TempDir) -> EngineConfig {
        EngineConfig {
            project_root: tmp.path().to_path_buf(),
            settings_path: tmp.path().join("settings.json"),
            db_path: tmp.path().join("state.db"),
            skills_dir: tmp.path().join("skills"),
            global_config_path: tmp.path().join(".novel-agent/api_config.json"),
        }
    }

    #[tokio::test]
    async fn offline_turn_produces_assistant_reply() {
        let _offline = crate::test_env::StripDeepseekApiKey::new();
        let tmp = TempDir::new().unwrap();
        std::fs::create_dir_all(tmp.path().join("skills")).unwrap();
        let mut engine = AgentEngine::new(test_config(&tmp)).unwrap();
        engine.handle_message("测试消息").await.unwrap();
        assert!(engine
            .messages
            .iter()
            .any(|m| m.role == "assistant" && m.content.contains("测试消息")));
    }

    #[tokio::test]
    async fn messages_persisted_to_db() {
        let _offline = crate::test_env::StripDeepseekApiKey::new();
        let tmp = TempDir::new().unwrap();
        std::fs::create_dir_all(tmp.path().join("skills")).unwrap();
        let mut engine = AgentEngine::new(test_config(&tmp)).unwrap();
        engine.handle_message("持久化").await.unwrap();
        let stored = engine
            .shared
            .session
            .db
            .get_session_messages(&engine.shared.session.id, None)
            .unwrap();
        assert!(stored.len() >= 2);
    }

    #[tokio::test]
    async fn default_hooks_do_not_enqueue_tasks() {
        use crate::hooks::{default_hook_config, knowledge_auditor_hook_task};
        let hooks = default_hook_config();
        let input = serde_json::json!({"file_path": "chapters/chapter-001.md"});
        assert!(knowledge_auditor_hook_task(&hooks, "Write", Some(&input), "written").is_none());
    }

    #[tokio::test]
    async fn answer_question_clears_pending_and_continues() {
        let _offline = crate::test_env::StripDeepseekApiKey::new();
        let tmp = TempDir::new().unwrap();
        std::fs::create_dir_all(tmp.path().join("skills")).unwrap();
        let mut engine = AgentEngine::new(test_config(&tmp)).unwrap();
        engine.pending_user_question = Some("q1".into());
        engine
            .messages
            .push(tool_result_message("q1", "等待用户回答问题后再继续。"));
        engine
            .answer_question("q1", serde_json::json!({"selections": {}}), None)
            .await
            .unwrap();
        assert!(engine.pending_user_question.is_none());
    }

    #[tokio::test]
    async fn deny_tool_does_not_continue_while_question_pending() {
        let _offline = crate::test_env::StripDeepseekApiKey::new();
        let tmp = TempDir::new().unwrap();
        std::fs::create_dir_all(tmp.path().join("skills")).unwrap();
        let mut engine = AgentEngine::new(test_config(&tmp)).unwrap();
        engine.pending_user_question = Some("ask-q1".into());
        engine.pending_tools.insert(
            "write-1".into(),
            ToolCallSpec {
                id: "write-1".into(),
                name: "Write".into(),
                input: serde_json::json!({"file_path": "notes.md", "content": "x"}),
            },
        );
        let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();
        engine.deny_tool("write-1", None, Some(&tx)).await.unwrap();
        assert!(engine.pending_user_question.is_some());
        let mut saw_turn_complete = false;
        while let Ok(ev) = rx.try_recv() {
            if matches!(ev, Event::TurnComplete { .. }) {
                saw_turn_complete = true;
            }
        }
        assert!(!saw_turn_complete);
    }

    #[test]
    fn inject_sub_agent_report_allocates_sequence_after_user_message() {
        let tmp = TempDir::new().unwrap();
        std::fs::create_dir_all(tmp.path().join("skills")).unwrap();
        let mut engine = AgentEngine::new(test_config(&tmp)).unwrap();
        engine.turn_number = 1;
        engine.turn_message_seq = 0;

        let user_msg = ChatMessage {
            role: "user".into(),
            content: "run consistency check".into(),
            tool_call_id: None,
            tool_calls: None,
            reasoning_content: None,
        };
        engine
            .persist_message_at_seq(&user_msg, MSG_SEQ_USER)
            .unwrap();
        engine.messages.push(user_msg);

        let assistant = ChatMessage {
            role: "assistant".into(),
            content: "forking sub-agent".into(),
            tool_call_id: None,
            tool_calls: None,
            reasoning_content: None,
        };
        engine.persist_message_alloc(&assistant).unwrap();
        engine.messages.push(assistant);

        engine
            .inject_sub_agent_report(AgentType::KnowledgeAuditor, "POV ok", None)
            .unwrap();

        let stored = engine
            .shared
            .session
            .db
            .get_session_messages(&engine.shared.session.id, None)
            .unwrap();
        let turn_one: Vec<_> = stored.iter().filter(|m| m.turn_number == 1).collect();
        assert_eq!(turn_one.len(), 3);
        assert_eq!(turn_one[0].sequence, MSG_SEQ_USER);
        assert_eq!(turn_one[1].sequence, 1);
        assert_eq!(turn_one[2].sequence, 2);
        assert!(turn_one[2]
            .content_json
            .to_string()
            .contains("子 Agent 完成"));
    }

    #[test]
    fn build_message_rows_keeps_sub_agent_report_in_same_turn() {
        let tmp = TempDir::new().unwrap();
        std::fs::create_dir_all(tmp.path().join("skills")).unwrap();
        let mut engine = AgentEngine::new(test_config(&tmp)).unwrap();
        engine.turn_number = 1;
        engine.messages.clear();
        engine.messages.push(ChatMessage {
            role: "system".into(),
            content: "sys".into(),
            tool_call_id: None,
            tool_calls: None,
            reasoning_content: None,
        });
        engine.messages.push(ChatMessage {
            role: "user".into(),
            content: "hello".into(),
            tool_call_id: None,
            tool_calls: None,
            reasoning_content: None,
        });
        engine.messages.push(ChatMessage {
            role: "assistant".into(),
            content: "working".into(),
            tool_call_id: None,
            tool_calls: None,
            reasoning_content: None,
        });
        engine.messages.push(ChatMessage {
            role: "user".into(),
            content: format!(
                "{} KnowledgeAuditor]\nreport",
                AgentEngine::SUB_AGENT_REPORT_PREFIX
            ),
            tool_call_id: None,
            tool_calls: None,
            reasoning_content: None,
        });

        let rows = engine.build_message_rows();
        assert_eq!(rows.len(), 4);
        assert_eq!(rows[0].0, 0);
        assert_eq!(rows[0].1, 0);
        assert_eq!(rows[1].0, 1);
        assert_eq!(rows[1].1, MSG_SEQ_USER);
        assert_eq!(rows[2].0, 1);
        assert_eq!(rows[2].1, 1);
        assert_eq!(rows[3].0, 1);
        assert_eq!(rows[3].1, 2);
    }

    #[tokio::test]
    async fn drain_subagent_jobs_injects_report_with_unique_sequences() {
        let _offline = crate::test_env::StripDeepseekApiKey::new();
        let tmp = TempDir::new().unwrap();
        std::fs::create_dir_all(tmp.path().join("skills")).unwrap();
        let mut engine = AgentEngine::new(test_config(&tmp)).unwrap();
        engine.turn_number = 1;
        engine.turn_message_seq = 0;

        let user_msg = ChatMessage {
            role: "user".into(),
            content: "fork consistency check".into(),
            tool_call_id: None,
            tool_calls: None,
            reasoning_content: None,
        };
        engine
            .persist_message_at_seq(&user_msg, MSG_SEQ_USER)
            .unwrap();
        engine.messages.push(user_msg);

        let tool_msg = tool_result_message("tc-fork", "Subagent 已启动");
        engine.persist_message_alloc(&tool_msg).unwrap();
        engine.messages.push(tool_msg);

        {
            let mut guard = engine
                .shared
                .subagent_queue
                .lock()
                .expect("subagent queue lock");
            guard.push(PendingSubagentWork {
                agent_type: "KnowledgeAuditor".into(),
                task: "审计 chapters/chapter-001.md".into(),
                parent_tool_call_id: Some("tc-fork".into()),
            });
        }

        drain_subagent_jobs(&mut engine, None).await.unwrap();

        let stored = engine
            .shared
            .session
            .db
            .get_session_messages(&engine.shared.session.id, None)
            .unwrap();
        let turn_one: Vec<_> = stored.iter().filter(|m| m.turn_number == 1).collect();
        assert!(
            turn_one
                .iter()
                .any(|m| m.content_json.to_string().contains("子 Agent 完成")),
            "expected sub-agent report in DB after sync drain"
        );
        let mut seen = std::collections::HashSet::new();
        for m in &turn_one {
            assert!(
                seen.insert((m.turn_number, m.sequence)),
                "duplicate (turn, sequence)=({}, {})",
                m.turn_number,
                m.sequence
            );
        }
        assert_eq!(
            engine
                .shared
                .sub_agent_count
                .load(std::sync::atomic::Ordering::SeqCst),
            0
        );
    }

    #[tokio::test]
    async fn parent_llm_context_excludes_fork_transcript() {
        let _offline = crate::test_env::StripDeepseekApiKey::new();
        let tmp = TempDir::new().unwrap();
        std::fs::create_dir_all(tmp.path().join("skills")).unwrap();
        let mut engine = AgentEngine::new(test_config(&tmp)).unwrap();
        engine.turn_number = 1;
        engine.turn_message_seq = 0;

        let user_msg = ChatMessage {
            role: "user".into(),
            content: "fork isolation check".into(),
            tool_call_id: None,
            tool_calls: None,
            reasoning_content: None,
        };
        engine
            .persist_message_at_seq(&user_msg, MSG_SEQ_USER)
            .unwrap();
        engine.messages.push(user_msg);

        let tool_msg = tool_result_message("tc-fork-iso", "Subagent 已启动");
        engine.persist_message_alloc(&tool_msg).unwrap();
        engine.messages.push(tool_msg);

        {
            let mut guard = engine
                .shared
                .subagent_queue
                .lock()
                .expect("subagent queue lock");
            guard.push(PendingSubagentWork {
                agent_type: "KnowledgeAuditor".into(),
                task: "审计 chapters/chapter-001.md".into(),
                parent_tool_call_id: Some("tc-fork-iso".into()),
            });
        }

        drain_subagent_jobs(&mut engine, None).await.unwrap();

        let stored = engine
            .shared
            .session
            .db
            .get_session_messages(&engine.shared.session.id, None)
            .unwrap();
        let report = stored
            .iter()
            .filter(|m| m.role != "system")
            .find(|m| m.content_json.to_string().contains("子 Agent 完成"))
            .expect("sub-agent report in parent session");
        let fork_run_id = report
            .content_json
            .get("fork_run_id")
            .and_then(|v| v.as_str())
            .expect("fork_run_id metadata on report");

        let fork_msgs = engine
            .shared
            .session
            .db
            .get_fork_messages(fork_run_id)
            .unwrap();
        assert!(
            !fork_msgs.is_empty(),
            "fork transcript should be persisted separately"
        );

        let parent_llm: String = crate::message_bridge::to_llm_messages(&engine.messages)
            .iter()
            .map(|m| format!("{:?}", m))
            .collect();
        assert!(
            parent_llm.contains("子 Agent 完成"),
            "tool-path report summary must remain in parent LLM context"
        );

        for fm in &fork_msgs {
            if fm.role != "tool" {
                continue;
            }
            if let Some(text) = fm.content_json.get("content").and_then(|c| c.as_str()) {
                if text.len() > 24 && !text.starts_with("Error:") {
                    assert!(
                        !parent_llm.contains(text),
                        "fork tool result leaked into parent LLM context: {text}"
                    );
                }
            }
        }
    }

    #[tokio::test]
    async fn drain_subagent_jobs_injects_multiple_reports_in_order() {
        let _offline = crate::test_env::StripDeepseekApiKey::new();
        let tmp = TempDir::new().unwrap();
        std::fs::create_dir_all(tmp.path().join("skills")).unwrap();
        let mut engine = AgentEngine::new(test_config(&tmp)).unwrap();
        engine.turn_number = 1;
        engine.turn_message_seq = 0;

        let user_msg = ChatMessage {
            role: "user".into(),
            content: "parallel fork".into(),
            tool_call_id: None,
            tool_calls: None,
            reasoning_content: None,
        };
        engine
            .persist_message_at_seq(&user_msg, MSG_SEQ_USER)
            .unwrap();
        engine.messages.push(user_msg);
        let tool_msg = tool_result_message("tc-fork-batch", "batch started");
        engine.persist_message_alloc(&tool_msg).unwrap();
        engine.messages.push(tool_msg);

        {
            let mut guard = engine
                .shared
                .subagent_queue
                .lock()
                .expect("subagent queue lock");
            guard.push(PendingSubagentWork {
                agent_type: "KnowledgeAuditor".into(),
                task: "任务 A：chapter-001".into(),
                parent_tool_call_id: Some("tc-fork-a".into()),
            });
            guard.push(PendingSubagentWork {
                agent_type: "ChapterCraftAnalyzer".into(),
                task: "任务 B：chapter-001".into(),
                parent_tool_call_id: Some("tc-fork-b".into()),
            });
        }

        drain_subagent_jobs(&mut engine, None).await.unwrap();

        let stored = engine
            .shared
            .session
            .db
            .get_session_messages(&engine.shared.session.id, None)
            .unwrap();
        let reports: Vec<_> = stored
            .iter()
            .filter(|m| m.role != "system" && m.content_json.to_string().contains("子 Agent 完成"))
            .collect();
        assert_eq!(reports.len(), 2);
        assert_eq!(reports[0].sequence, 2);
        assert_eq!(reports[1].sequence, 3);
        assert!(reports[0]
            .content_json
            .to_string()
            .contains("KnowledgeAuditor"));
        assert!(reports[1]
            .content_json
            .to_string()
            .contains("ChapterCraftAnalyzer"));
    }

    #[test]
    fn resume_inner_turn_counts_existing_assistants() {
        let tmp = TempDir::new().unwrap();
        let engine = AgentEngine::new(test_config(&tmp)).unwrap();
        assert_eq!(engine.resume_inner_turn_from_messages(), 0);

        let mut engine = engine;
        engine.messages.push(ChatMessage {
            role: "assistant".into(),
            content: "hi".into(),
            tool_call_id: None,
            tool_calls: Some(vec![crate::ToolCallRecord {
                id: "tc1".into(),
                name: "AskUserQuestion".into(),
                arguments: serde_json::json!({}),
            }]),
            reasoning_content: None,
        });
        assert_eq!(engine.resume_inner_turn_from_messages(), 1);
    }

    #[test]
    fn should_continue_inner_after_reasoning_only_completion() {
        let c = LlmCompletion {
            content: None,
            reasoning_content: Some("plan WebSearch next".into()),
            tool_calls: vec![],
            usage: None,
            stop_reason: Some("stop".into()),
        };
        assert!(should_continue_inner_after_completion(&c));

        let with_tools = LlmCompletion {
            tool_calls: vec![LlmToolCall {
                id: "t1".into(),
                name: "WebSearch".into(),
                arguments: "{}".into(),
            }],
            ..Default::default()
        };
        assert!(!should_continue_inner_after_completion(&with_tools));

        let with_text = LlmCompletion {
            content: Some("done".into()),
            reasoning_content: Some("thought".into()),
            ..Default::default()
        };
        assert!(!should_continue_inner_after_completion(&with_text));
    }

    #[tokio::test]
    async fn compact_and_sync_clears_read_file_cache() {
        use novel_tools::{ReadCacheEntry, ReadCacheSource};
        use std::path::PathBuf;

        let tmp = TempDir::new().unwrap();
        std::fs::create_dir_all(tmp.path().join("skills")).unwrap();
        let mut engine = AgentEngine::new(test_config(&tmp)).unwrap();
        engine.messages.push(ChatMessage {
            role: "user".into(),
            content: "chapter work".into(),
            tool_call_id: None,
            tool_calls: None,
            reasoning_content: None,
        });
        engine.messages.push(ChatMessage {
            role: "assistant".into(),
            content: "ok".into(),
            tool_call_id: None,
            tool_calls: None,
            reasoning_content: None,
        });
        engine.last_context_tokens = 850_000;

        engine.shared.read_file_cache.insert(
            PathBuf::from("chapters/ch01.md"),
            ReadCacheEntry {
                mtime_secs: 1,
                raw_content: "line".into(),
                offset: None,
                limit: None,
                total_lines: 1,
                source: ReadCacheSource::Read,
            },
        );
        assert_eq!(engine.shared.read_file_cache.len(), 1);

        engine.compact_and_sync(None).await.unwrap();
        assert!(engine.shared.read_file_cache.is_empty());
    }

    #[tokio::test]
    async fn compact_and_sync_skipped_when_under_threshold_keeps_read_cache() {
        use novel_tools::{ReadCacheEntry, ReadCacheSource};
        use std::path::PathBuf;

        let tmp = TempDir::new().unwrap();
        std::fs::create_dir_all(tmp.path().join("skills")).unwrap();
        let mut engine = AgentEngine::new(test_config(&tmp)).unwrap();
        engine.last_context_tokens = 0;

        engine.shared.read_file_cache.insert(
            PathBuf::from("chapters/ch01.md"),
            ReadCacheEntry {
                mtime_secs: 1,
                raw_content: "line".into(),
                offset: None,
                limit: None,
                total_lines: 1,
                source: ReadCacheSource::Read,
            },
        );

        engine.compact_and_sync(None).await.unwrap();
        assert_eq!(engine.shared.read_file_cache.len(), 1);
    }
}
