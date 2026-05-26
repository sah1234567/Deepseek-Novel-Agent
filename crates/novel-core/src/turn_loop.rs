use crate::dynamic_context::{
    filter_loadable_reference_paths, filter_loadable_skill_ids, format_activated_skill_block,
    parse_skill_reference_path,
};
use crate::interrupt::{AbortController, InterruptReason, ERROR_MESSAGE_USER_ABORT};
use crate::message_bridge::{
    assistant_from_completion, chat_slice_to_compaction, chat_to_compaction,
    chat_to_json,
    compaction_slice_to_chat, parse_tool_call_input, to_llm_messages,
    tool_result_message,
};
use crate::messages::{create_user_interruption_message, yield_missing_tool_result_blocks};
use crate::subagent_react::{
    react_limit_reminder_message, report_only_tool_rejection, SubagentLoopPhase,
};
use crate::subagent_overflow::{
    build_partial_report, task_preview_120, OVERFLOW_KIND_INPUT_REJECTED,
    OVERFLOW_KIND_OUTPUT_TRUNCATED,
};
use crate::turn::TurnContext;
use crate::{
    hooks::tool_schemas_for_agent, AgentEngine, AgentError, AgentType, ChatMessage,
    CompactionAction, ContentBlockKind, Event, ForkError, ForkedAgentContext,
    TerminalReason,
};
use novel_deepseek::{
    is_context_length_exceeded, is_output_truncated, ChatClient, LlmChatMessage, LlmCompletion,
    LlmError, LlmToolCall, StreamEvent, StreamOutcome,
};
use novel_logging::LogEvent;
use crate::turn::MSG_SEQ_USER;
use novel_tools::{
    AbortSignal, PermissionMode, PermissionResult, StreamingToolExecutor, ToolCallSpec,
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

struct StreamingToolDispatch {
    executor: Option<StreamingToolExecutor>,
    handled_ids: HashSet<String>,
    executed_specs: Vec<ToolCallSpec>,
    pending_specs: HashMap<String, ToolCallSpec>,
    denied_specs: HashMap<String, (ToolCallSpec, String)>,
    ui_result_emitted: HashSet<String>,
}

impl StreamingToolDispatch {
    fn new(
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

    fn take_executor(&mut self) -> StreamingToolExecutor {
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

    fn handle_ready(
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
            Some(Err(format!(
                "Invalid tool arguments JSON for {}",
                tc.name
            )))
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
            self.denied_specs
                .insert(spec.id.clone(), (spec, reason));
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

    fn discard(&mut self) {
        if let Some(executor) = self.executor.as_mut() {
            executor.discard();
        }
    }
}

impl AgentEngine {
    pub fn init_llm(&mut self) {
        if self.llm.is_some() {
            return;
        }
        // Priority: DEEPSEEK_API_KEY (env) > agent api_config.json > offline mock
        let api_key = std::env::var("DEEPSEEK_API_KEY")
            .ok()
            .or_else(|| {
                novel_config::load_agent_api_config(&self.shared.global_config_path)
                    .ok()
                    .flatten()
                    .and_then(|c| if c.api_key.is_empty() { None } else { Some(c.api_key) })
            });
        let api_base = std::env::var("DEEPSEEK_API_BASE")
            .ok()
            .or_else(|| {
                novel_config::load_agent_api_config(&self.shared.global_config_path)
                    .ok()
                    .flatten()
                    .map(|c| c.api_base)
                    .filter(|b| !b.is_empty())
            })
            .unwrap_or_else(|| novel_deepseek::chat_api_base());
        let model = &self.shared.settings.model.model;
        self.llm = match api_key {
            Some(key) => Some(ChatClient::deepseek(&key, model, &api_base, self.shared.settings.model.thinking_enabled)),
            None => ChatClient::from_env(model).ok(),
        };
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
        event_tx: Option<&mpsc::UnboundedSender<Event>>,
    ) -> Result<TerminalReason, AgentError> {
        if content.trim().is_empty() {
            tracing::warn!("handle_message rejected: empty content");
            return Err(AgentError::Validation("empty message".into()));
        }
        if self.hook_running {
            tracing::warn!(session_id = %self.shared.session.id, "handle_message rejected: hook running");
            return Err(AgentError::AgentBusy);
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

        let (hit, miss, comp) = self.session_token_summary();
        let (th, tm, tc) = self
            .last_turn_usage
            .as_ref()
            .map(|u| {
                (
                    u.cache_hit_tokens,
                    u.cache_miss_tokens,
                    u.completion_tokens,
                )
            })
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
                let _ = tx.send(Event::Error {
                    message: "用户已中断".into(),
                    recoverable: true,
                });
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

    // ── Forked agent turn ─────────────────────────────────────

    pub async fn run_forked_agent(
        &mut self,
        child: &mut crate::ForkedAgentContext,
        fork_run_id: &str,
        event_tx: Option<&mpsc::UnboundedSender<Event>>,
    ) -> Result<String, AgentError> {
        if self.shared.sub_agent_count.load(std::sync::atomic::Ordering::SeqCst) > 0 {
            return Err(AgentError::NestedForkProhibited);
        }
        self.sub_agent_inc();
        let result = self
            .run_forked_agent_inner(child, fork_run_id, event_tx)
            .await;
        self.sub_agent_dec();
        result
    }

    async fn run_forked_agent_inner(
        &mut self,
        child: &mut crate::ForkedAgentContext,
        fork_run_id: &str,
        event_tx: Option<&mpsc::UnboundedSender<Event>>,
    ) -> Result<String, AgentError> {
        tracing::debug!(
            session_id = %self.shared.session.id,
            agent_type = ?child.fork.agent_def.agent_type,
            max_react_loops = child.fork.max_react_loops,
            %fork_run_id,
            "forked_agent_start"
        );
        let task_preview: String = child.fork.task_message.content.chars().take(80).collect();
        let task_preview = if child.fork.task_message.content.chars().count() > 80 {
            format!("{task_preview}…")
        } else {
            task_preview
        };
        if let Some(tx) = event_tx {
            let _ = tx.send(Event::SubAgentStarted {
                fork_run_id: fork_run_id.to_string(),
                agent_id: child.fork.agent_def.agent_type.to_string(),
                agent_type: child.fork.agent_def.agent_type.to_string(),
                task_preview,
            });
        }
        // Persist task message (skip system — not shown in overlay).
        crate::fork_transcript::persist_fork_message(
            &self.shared.session.db,
            fork_run_id,
            &child.fork.task_message,
        )?;
        self.init_llm();
        self.active_sub_agent = Some(child.fork.agent_def.agent_type);
        let allowed = child.fork.agent_def.tools.clone();
        let schemas = tool_schemas_for_agent(&self.shared.registry, &allowed);
        let max_react_loops = child.fork.max_react_loops;
        let mut turn_ctx = TurnContext::new(1, max_react_loops);
        let mut phase = SubagentLoopPhase::Reacting;
        let task_text = child.fork.task_message.content.clone();
        loop {
            if self.interrupt_requested() {
                self.active_sub_agent = None;
                return Ok("子 Agent 已中断".into());
            }
            if matches!(phase, SubagentLoopPhase::Reacting) && !turn_ctx.needs_continuation() {
                let spent = turn_ctx.inner_spent();
                let reminder = react_limit_reminder_message(spent, max_react_loops);
                child.messages.push(reminder);
                phase = phase.enter_report_only();
            }
            let active_schemas = if phase.is_report_only() {
                &[] as &[(String, String, serde_json::Value)]
            } else {
                &schemas[..]
            };
            let snapshot = child.messages.clone();
            let llm_msgs = to_llm_messages(&child.messages);
            match self
                .call_llm_and_execute(
                    &llm_msgs,
                    active_schemas,
                    &mut turn_ctx,
                    event_tx,
                    Some(&mut child.messages),
                    false,
                    Some(fork_run_id),
                )
                .await
            {
                Err(AgentError::Llm(e)) if is_context_length_exceeded(&e) => {
                    child.messages = snapshot;
                    self.active_sub_agent = None;
                    return Ok(build_partial_report(
                        &child.fork.agent_def.agent_type.to_string(),
                        &task_preview_120(&task_text),
                        OVERFLOW_KIND_INPUT_REJECTED,
                    ));
                }
                Err(e) => return Err(e),
                Ok(LlmCallOutcome::Aborted(_)) => {
                    self.active_sub_agent = None;
                    return Ok("子 Agent 已中断".into());
                }
                Ok(LlmCallOutcome::Continue(completion)) => {
                    self.record_usage(&completion);
                    if is_output_truncated(completion.stop_reason.as_deref()) {
                        child.messages.push(assistant_from_completion(&completion));
                        self.active_sub_agent = None;
                        return Ok(build_partial_report(
                            &child.fork.agent_def.agent_type.to_string(),
                            &task_preview_120(&task_text),
                            OVERFLOW_KIND_OUTPUT_TRUNCATED,
                        ));
                    }
                    let tool_calls = completion.tool_calls.clone();
                    if tool_calls.is_empty() {
                        child.messages.push(assistant_from_completion(&completion));
                        break;
                    }
                    if phase.is_report_only() {
                        child.messages.push(assistant_from_completion(&completion));
                        for tc in &tool_calls {
                            child.messages.push(report_only_tool_rejection(&tc.id));
                        }
                        if let Some(next) = phase.consume_grace() {
                            phase = next;
                            continue;
                        }
                        break;
                    }
                    if let Err(TerminalReason::MaxReactLoops(_)) = turn_ctx.increment_inner() {
                        let spent = turn_ctx.inner_spent();
                        child.messages.push(react_limit_reminder_message(
                            spent,
                            max_react_loops,
                        ));
                        phase = phase.enter_report_only();
                        continue;
                    }
                }
            }
        }
        self.active_sub_agent = None;
        let output = child
            .messages
            .iter()
            .rev()
            .find(|m| m.role == "assistant")
            .map(|m| m.content.clone())
            .unwrap_or_else(|| "子 Agent 已完成".into());
        tracing::debug!(
            session_id = %self.shared.session.id,
            agent_type = ?child.fork.agent_def.agent_type,
            output_len = output.len(),
            "forked_agent_complete"
        );
        if let Some(tx) = event_tx {
            let _ = tx.send(Event::SubAgentComplete {
                fork_run_id: fork_run_id.to_string(),
                agent_id: child.fork.agent_def.agent_type.to_string(),
                output: output.clone(),
                cache_hit_rate: 0.0,
            });
        }
        Ok(output)
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

    /// Post-fork pipeline: inject sub-agent report into parent session for LLM to continue.
    pub async fn run_post_fork_pipeline(
        &mut self,
        agent_type: AgentType,
        output: &str,
        fork_run_id: Option<&str>,
    ) -> Result<(), AgentError> {
        self.inject_sub_agent_report(agent_type, output, fork_run_id)?;
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

            let llm_msgs = to_llm_messages(&self.messages);
            tracing::debug!(
                inner_turn = turn_ctx.inner_turn,
                message_count = self.messages.len(),
                llm_message_count = llm_msgs.len(),
                tool_schema_count = schemas.len(),
                "inner_turn_iteration"
            );

            let completion = match self
                .call_llm_and_execute(&llm_msgs, &schemas, turn_ctx, event_tx, None, true, None)
                .await?
            {
                LlmCallOutcome::Aborted(r) => return Ok(r),
                LlmCallOutcome::Continue(c) => c,
            };

            if self.pending_user_question.is_some() {
                // Assistant (+ tool stub) already persisted in call_llm_and_execute.
                return Ok(TerminalReason::Completed);
            }

            self.drain_pending_forks(event_tx).await?;
            self.drain_pending_hooks(event_tx).await?;

            self.record_usage(&completion);

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
                Err(TerminalReason::MaxReactLoops(n)) => return Ok(TerminalReason::MaxReactLoops(n)),
                Err(other) => return Ok(other),
            }
        }
    }

    // ── LLM call + streaming tool execution (unified path) ────

    fn append_interrupt_message(&mut self, tool_use: bool) -> Result<(), AgentError> {
        if self.abort_reason() == Some(InterruptReason::SubmitInterrupt) {
            return Ok(());
        }
        let msg = create_user_interruption_message(tool_use);
        self.persist_message_alloc(&msg)?;
        self.messages.push(msg);
        Ok(())
    }

    fn persist_partial_assistant(
        &mut self,
        completion: &LlmCompletion,
    ) -> Result<ChatMessage, AgentError> {
        let assistant = assistant_from_completion(completion);
        if assistant.content.is_empty() && assistant.tool_calls.is_none() {
            return Ok(assistant);
        }
        self.persist_message_alloc(&assistant)?;
        self.messages.push(assistant.clone());
        Ok(assistant)
    }

    async fn call_llm_and_execute(
        &mut self,
        messages: &[LlmChatMessage],
        tools: &[(String, String, serde_json::Value)],
        turn_ctx: &TurnContext,
        event_tx: Option<&mpsc::UnboundedSender<Event>>,
        mut message_sink: Option<&mut Vec<ChatMessage>>,
        persist_tool_messages: bool,
        fork_run_id: Option<&str>,
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
                                        kind: match kind {
                                            novel_deepseek::ContentBlockKind::Text => {
                                                ContentBlockKind::Text
                                            }
                                            novel_deepseek::ContentBlockKind::Thinking => {
                                                ContentBlockKind::Thinking
                                            }
                                            novel_deepseek::ContentBlockKind::ToolCall => {
                                                ContentBlockKind::ToolCall
                                            }
                                        },
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
                // Only purpose: keep session prompt_tokens count correct.
                //
                // The original request's cache_hit / cache_miss / completion
                // are lost (stream cancelled before usage chunk). Drain is a
                // *separate* request whose prompt_tokens happens to match
                // (same messages), so session total stays accurate. But the
                // three-class breakdown being recorded is drain's own, not the
                // original's — cache_hit is inflated (original primed cache),
                // completion_tokens is 1 (drain's). Fall back to partial.usage
                // (SSE chunk) when drain fails. Mutual exclusion avoids
                // double-counting.
                let effective_usage = if let Some(rx) = bg_usage {
                    match tokio::time::timeout(Duration::from_secs(1), rx).await {
                        Ok(Ok(Some(usage))) => Some(usage),
                        _ => completion.usage.clone(),
                    }
                } else {
                    completion.usage.clone()
                };
                if let Some(ref u) = effective_usage {
                    self.last_turn_usage = Some(u.clone());
                    let _ = self.shared.session.db.add_session_tokens(
                        &self.shared.session.id,
                        u.cache_hit_tokens,
                        u.cache_miss_tokens,
                        u.completion_tokens,
                    );
                    tracing::debug!(
                        cache_hit = u.cache_hit_tokens,
                        cache_miss = u.cache_miss_tokens,
                        completion = u.completion_tokens,
                        "token_usage_recorded_stream_abort"
                    );
                    self.audit_log(LogEvent::TokenAudit {
                        session_id: self.shared.session.id.clone(),
                        cache_hit_tokens: u.cache_hit_tokens,
                        cache_miss_tokens: u.cache_miss_tokens,
                        completion_tokens: u.completion_tokens,
                    });
                }
                let assistant = self.persist_partial_assistant(&completion)?;
                if !completion.tool_calls.is_empty() {
                    for tool_msg in
                        yield_missing_tool_result_blocks(&assistant, "Interrupted by user")
                    {
                        self.persist_message_alloc(&tool_msg)?;
                        self.messages.push(tool_msg);
                    }
                }
                self.append_interrupt_message(!completion.tool_calls.is_empty())?;
                return Ok(LlmCallOutcome::Aborted(TerminalReason::AbortedStreaming));
            }

            tracing::debug!(
                session_id = %session_id,
                tool_call_count = completion.tool_calls.len(),
                has_usage = completion.usage.is_some(),
                stream_aborted,
                "llm_request_complete"
            );

            if let Some(tx) = event_tx {
                let _ = tx.send(Event::AssistantSegmentComplete {
                    segment_index: turn_ctx.inner_turn,
                    fork_run_id: fork_run_id.map(str::to_string),
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
                        let needs_approval = self
                            .pending_tools
                            .contains_key(&tc.id)
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
                results.push((
                    id,
                    Err(novel_tools::ToolError::PermissionDenied(reason)),
                ));
            }
            self.has_interruptible_tool_in_progress = false;
            for spec in &executed_specs {
                self.pending_tools.remove(&spec.id);
            }

            let assistant = assistant_from_completion(&completion);
            if let Some(sink) = message_sink.as_deref_mut() {
                if let Some(run_id) = fork_run_id {
                    crate::fork_transcript::persist_fork_message(
                        &self.shared.session.db,
                        run_id,
                        &assistant,
                    )?;
                }
                sink.push(assistant);
            } else {
                self.persist_message_alloc(&assistant)?;
                self.messages.push(assistant);
            }

            let tool_call_order: Vec<String> =
                completion.tool_calls.iter().map(|tc| tc.id.clone()).collect();

            if self.interrupt_requested() {
                let _ = self
                    .execute_stream_results(
                        results,
                        &executed_specs,
                        &tool_call_order,
                        &skip_result_events,
                        event_tx,
                        message_sink.as_deref_mut(),
                        persist_tool_messages,
                        fork_run_id,
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
                    message_sink.as_deref_mut(),
                    persist_tool_messages,
                    fork_run_id,
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
                    fork_run_id: fork_run_id.map(str::to_string),
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
        mut message_sink: Option<&mut Vec<ChatMessage>>,
        persist_tool_messages: bool,
        fork_run_id: Option<&str>,
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
                    let incoming_needs_input = matches!(
                        &result,
                        Err(novel_tools::ToolError::NeedsUserInput { .. })
                    );
                    let existing_needs_input = matches!(
                        existing,
                        Err(novel_tools::ToolError::NeedsUserInput { .. })
                    );
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
                        if let Some(run_id) = fork_run_id {
                            let _ = tx.send(Event::SubAgentToolUpdate {
                                fork_run_id: run_id.to_string(),
                                phase: "result".into(),
                                tool_call_id: id.clone(),
                                tool_name: spec.map(|s| s.name.clone()),
                                input: None,
                                content: Some(content.clone()),
                                needs_approval: None,
                                status: None,
                                description: None,
                            });
                        } else {
                            let _ = tx.send(Event::ToolCallResult {
                                tool_call_id: id.clone(),
                                content: content.clone(),
                            });
                        }
                    }
                    let tool_msg = tool_result_message(&id, &content);
                    if persist_tool_messages {
                        self.persist_message_alloc(&tool_msg)?;
                    }
                    if let Some(run_id) = fork_run_id {
                        crate::fork_transcript::persist_fork_message(
                            &self.shared.session.db,
                            run_id,
                            &tool_msg,
                        )?;
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
                    if let Some(sink) = message_sink.as_mut() {
                        sink.push(tool_msg);
                    } else {
                        self.messages.push(tool_msg);
                    }

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
                        if !self.is_forked_child && s.name == "Read" && success {
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
                                        let _ = self
                                            .shared
                                            .session
                                            .db
                                            .set_read_skill_reference_paths(
                                                &self.shared.session.id,
                                                &self.read_skill_reference_paths,
                                            );
                                    }
                                }
                            }
                        }
                        // Opt-in PostToolUse hooks (settings.json); default config is empty.
                        // Matching is now handled by hook_config.matcher, not hardcoded tool names.
                        if !self.is_forked_child {
                            if let Some(task) = hook_task {
                                self.pending_hook_tasks.push(task);
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
                    if let Some(sink) = message_sink.as_mut() {
                        sink.push(tool_msg);
                    } else {
                        self.messages.push(tool_msg);
                    }
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
                    if let Some(sink) = message_sink.as_mut() {
                        sink.push(tool_msg);
                    } else {
                        self.messages.push(tool_msg);
                    }
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
        let spec = self
            .pending_tools
            .remove(tool_call_id)
            .ok_or_else(|| {
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
        let spec = self
            .pending_tools
            .remove(tool_call_id)
            .ok_or_else(|| {
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
        if self.hook_running {
            tracing::warn!("continue_turn_loop rejected: hook running");
            return Err(AgentError::AgentBusy);
        }
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
        let (hit, miss, comp) = self.session_token_summary();
        let (th, tm, tc) = self
            .last_turn_usage
            .as_ref()
            .map(|u| {
                (
                    u.cache_hit_tokens,
                    u.cache_miss_tokens,
                    u.completion_tokens,
                )
            })
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
                let _ = tx.send(Event::Error {
                    message: "用户已中断".into(),
                    recoverable: true,
                });
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
    fn compaction_needed(&self) -> bool {
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
        use novel_compaction::{build_summary_trailing_user_prompt, rule_based_summary, truncate_summary};

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
        if self.is_forked_child {
            return Ok(());
        }

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
        tracing::info!(
            tokens_before = self.last_context_tokens,
            "compaction_start"
        );
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

        self.rebuild_system_message()?;

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
        let _ = self.shared.session.db.set_invoked_skill_ids(
            &self.shared.session.id,
            &skill_ids,
        );
        self.read_skill_reference_paths = ref_paths.clone();
        let _ = self.shared.session.db.set_read_skill_reference_paths(
            &self.shared.session.id,
            &ref_paths,
        );

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
        tracing::info!(tokens_before, messages = self.messages.len(), "compaction_done");
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
    async fn compact_with_events(
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
            } else if msg.content.starts_with("[激活 Skill]") {
                (self.turn_number as i32, 97)
            } else if msg.content.starts_with("[会话历史摘要]")
                || msg.content.starts_with("[上下文刷新]")
            {
                (self.turn_number as i32, 98)
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

    fn persist_message_at_seq(&mut self, msg: &ChatMessage, sequence: i32) -> Result<(), AgentError> {
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
        if let Err(e) = self
            .shared
            .session
            .db
            .insert_message(
                &self.shared.session.id,
                self.turn_number as i32,
                sequence,
                &msg.role,
                &chat_to_json(msg),
                None,
            )
        {
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
            .max_message_sequence_for_turn(
                &self.shared.session.id,
                self.turn_number as i32,
            )
            .map_err(AgentError::State)?;
        self.turn_message_seq = max;
        Ok(())
    }

    fn persist_message_alloc(&mut self, msg: &ChatMessage) -> Result<(), AgentError> {
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

    fn record_usage(&mut self, completion: &LlmCompletion) {
        if let Some(u) = &completion.usage {
            self.last_turn_usage = Some(u.clone());
            let _ = self.shared.session.db.add_session_tokens(
                &self.shared.session.id,
                u.cache_hit_tokens,
                u.cache_miss_tokens,
                u.completion_tokens,
            );
            tracing::debug!(
                cache_hit = u.cache_hit_tokens,
                cache_miss = u.cache_miss_tokens,
                completion = u.completion_tokens,
                "token_usage_recorded"
            );
            self.audit_log(LogEvent::TokenAudit {
                session_id: self.shared.session.id.clone(),
                cache_hit_tokens: u.cache_hit_tokens,
                cache_miss_tokens: u.cache_miss_tokens,
                completion_tokens: u.completion_tokens,
            });
        } else {
            self.last_turn_usage = None;
        }
    }

    pub fn session_token_summary(&self) -> (i64, i64, i64) {
        self.shared.session
            .db
            .get_session(&self.shared.session.id)
            .ok()
            .flatten()
            .map(|s| (s.cache_hit_tokens, s.cache_miss_tokens, s.completion_tokens))
            .unwrap_or((0, 0, 0))
    }

    fn main_tool_names(&self) -> Vec<String> {
        self.shared.registry.names()
    }

    async fn drain_pending_hooks(
        &mut self,
        event_tx: Option<&mpsc::UnboundedSender<Event>>,
    ) -> Result<(), AgentError> {
        // PostToolUse auto-trigger path (source=`hook`): KnowledgeAuditor subagent, no parent inject.
        let tasks = std::mem::take(&mut self.pending_hook_tasks);
        if !tasks.is_empty() {
            tracing::debug!(hook_count = tasks.len(), "drain_pending_hooks");
        }
        for task in tasks {
            self.run_knowledge_auditor_hook(task, event_tx).await?;
        }
        Ok(())
    }

    async fn drain_pending_forks(
        &mut self,
        event_tx: Option<&mpsc::UnboundedSender<Event>>,
    ) -> Result<(), AgentError> {
        // ForkSubAgent tool path (source=`tool`): parallel spawn → join → one report inject per fork.
        // Full transcript in `fork_messages`; only summary enters parent `self.messages` / LLM context.
        let raw_pending: Vec<(String, String)> = {
            let mut guard = self
                .shared.fork_queue
                .lock()
                .map_err(|_| AgentError::Validation("fork queue lock poisoned".into()))?;
            std::mem::take(&mut *guard)
        };
        if raw_pending.is_empty() {
            return Ok(());
        }

        let mut pending: Vec<(AgentType, String)> = Vec::with_capacity(raw_pending.len());
        for (agent_name, task) in raw_pending {
            let agent_type = AgentType::parse(&agent_name).ok_or_else(|| {
                tracing::warn!(%agent_name, "drain_pending_forks: unknown agent type");
                AgentError::Validation(format!("unknown fork agentType: {agent_name}"))
            })?;
            pending.push((agent_type, task));
        }

        tracing::debug!(
            fork_count = pending.len(),
            "drain_pending_forks_sync_start"
        );

        let mut handles = Vec::with_capacity(pending.len());
        let mut fork_run_ids = Vec::with_capacity(pending.len());
        for (agent_type, task) in pending {
            tracing::debug!(
                ?agent_type,
                task_len = task.len(),
                "drain_pending_forks_spawn"
            );
            let fork_run_id = crate::fork_transcript::create_fork_run(
                &self.shared.session.db,
                &self.shared.session.id,
                self.turn_number as i32,
                &agent_type.to_string(),
                &task,
                "tool",
            )?;
            fork_run_ids.push(fork_run_id.clone());
            let shared = self.shared.clone();
            self.sub_agent_inc();
            let event_tx_clone = event_tx.cloned();
            handles.push(tokio::spawn(async move {
                let result = run_subagent_async(
                    shared,
                    agent_type,
                    task,
                    fork_run_id,
                    event_tx_clone,
                )
                .await;
                (
                    agent_type,
                    result.unwrap_or_else(|e| format!("子 Agent 错误: {e}")),
                )
            }));
        }

        let fork_count = handles.len();
        for (i, handle) in handles.into_iter().enumerate() {
            let fork_run_id = fork_run_ids
                .get(i)
                .cloned()
                .unwrap_or_default();
            match handle.await {
                Ok((agent_type, output)) => {
                    tracing::debug!(
                        ?agent_type,
                        output_len = output.len(),
                        %fork_run_id,
                        "drain_pending_forks_inject"
                    );
                    self.inject_sub_agent_report(agent_type, &output, Some(&fork_run_id))?;
                    if self.compaction_needed() {
                        self.compact_with_events(event_tx).await;
                    }
                }
                Err(e) => {
                    tracing::error!(error = %e, "drain_pending_forks_join_failed");
                    return Err(AgentError::Validation(format!(
                        "subagent task join failed: {e}"
                    )));
                }
            }
        }

        tracing::debug!(fork_count, "drain_pending_forks_sync_complete");
        Ok(())
    }
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

/// Run a sub-agent (ForkSubAgent tool path). Transcript → `fork_messages` only.
pub async fn run_subagent_async(
    shared: crate::EngineShared,
    agent_type: AgentType,
    task: String,
    fork_run_id: String,
    event_tx: Option<mpsc::UnboundedSender<Event>>,
) -> Result<String, AgentError> {
    tracing::debug!(
        session_id = %shared.session.id,
        ?agent_type,
        task_len = task.len(),
        "subagent_async_start"
    );
    // Init LLM from env/config (same logic as AgentEngine::init_llm)
    let api_key = std::env::var("DEEPSEEK_API_KEY")
        .ok()
        .or_else(|| {
            novel_config::load_agent_api_config(&shared.global_config_path)
                .ok()
                .flatten()
                .and_then(|c| if c.api_key.is_empty() { None } else { Some(c.api_key) })
        });
    let api_base = std::env::var("DEEPSEEK_API_BASE")
        .ok()
        .or_else(|| {
            novel_config::load_agent_api_config(&shared.global_config_path)
                .ok()
                .flatten()
                .map(|c| c.api_base)
                .filter(|b| !b.is_empty())
        })
        .unwrap_or_else(|| shared.settings.model.api_base.clone());
    let model = &shared.settings.model.model;
    let mut llm: Option<ChatClient> = match api_key {
        Some(key) => Some(ChatClient::deepseek(&key, model, &api_base, shared.settings.model.thinking_enabled)),
        None => ChatClient::from_env(model).ok(),
    };

    // Build fork context
    let system_msg = ChatMessage {
        role: "system".into(),
        content: shared.system_prompt.clone(),
        tool_call_id: None,
        tool_calls: None,
        reasoning_content: None,
    };
    let knowledge_snapshots = std::collections::HashMap::new();
    let mut child = ForkedAgentContext::fork(
        &system_msg,
        shared.session.id.clone(),
        agent_type,
        task.clone(),
        agent_type.max_react_loops_for(&shared.settings.agent),
        knowledge_snapshots,
        false,
    )
    .map_err(|e| {
        if e == ForkError::InvalidMaxReactLoops(0) {
            AgentError::NestedForkProhibited
        } else {
            AgentError::Fork(e)
        }
    })?;

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
        let llm_msgs = to_llm_messages(&child.messages);
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
            always_allow: shared.settings.permissions.always_allow.clone(),
            project_root: shared.session.project_root.clone(),
            session_id: shared.session.id.clone(),
            db: Some(Arc::new(shared.session.db.clone())),
            permission_mode_override: Some(Arc::clone(&shared.permission_mode_override)),
            read_file_cache: Some(Arc::clone(&shared.read_file_cache)),
            allow_fork: false,
            fork_queue: None,
            skills_dir: Some(shared.agent_skills_dir.clone()),
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
                                        kind: match kind {
                                            novel_deepseek::ContentBlockKind::Text => {
                                                ContentBlockKind::Text
                                            }
                                            novel_deepseek::ContentBlockKind::Thinking => {
                                                ContentBlockKind::Thinking
                                            }
                                            novel_deepseek::ContentBlockKind::ToolCall => {
                                                ContentBlockKind::ToolCall
                                            }
                                        },
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
                Ok(StreamOutcome::Cancelled { partial, background_usage }) => {
                    if let Ok(mut d) = dispatch_arc.lock() {
                        d.discard();
                    }
                    was_interrupted = true;
                    interrupt_reason = "流中断";
                    // Drain → keep session prompt_tokens correct. Three-class
                    // breakdown is drain's own, not the original request's.
                    let usage = match tokio::time::timeout(Duration::from_secs(1), background_usage).await {
                        Ok(Ok(Some(u))) => Some(u),
                        _ => partial.usage.clone(),
                    };
                    if let Some(u) = &usage {
                        let _ = shared.session.db.add_session_tokens(
                            &shared.session.id,
                            u.cache_hit_tokens,
                            u.cache_miss_tokens,
                            u.completion_tokens,
                        );
                    }
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
                    shared.sub_agent_count.fetch_sub(1, std::sync::atomic::Ordering::SeqCst);
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
                    interrupt_reason = "API 错误";
                    break;
                }
            }
        } else {
            ChatClient::offline_complete(&llm_msgs)
        };

        if is_output_truncated(completion.stop_reason.as_deref()) {
            tracing::warn!(
                session_id = %shared.session.id,
                ?agent_type,
                "subagent_output_truncated"
            );
            fork_child_push(
                &shared.session.db,
                &fork_run_id,
                &mut child.messages,
                assistant_from_completion(&completion),
            )?;
            shared.sub_agent_count.fetch_sub(1, std::sync::atomic::Ordering::SeqCst);
            return Ok(build_partial_report(
                &agent_type.to_string(),
                &task_preview_120(&task),
                OVERFLOW_KIND_OUTPUT_TRUNCATED,
            ));
        }

        if let Some(u) = &completion.usage {
            let _ = shared.session.db.add_session_tokens(
                &shared.session.id,
                u.cache_hit_tokens,
                u.cache_miss_tokens,
                u.completion_tokens,
            );
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
        fork_child_push(
            &shared.session.db,
            &fork_run_id,
            &mut child.messages,
            assistant_from_completion(&completion),
        )?;
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
        let task_preview: String = child.fork.task_message.content
            .chars().take(200).collect();
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
    shared.sub_agent_count.fetch_sub(1, std::sync::atomic::Ordering::SeqCst);

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
        "subagent_async_complete"
    );

    Ok(output)
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
            .shared.session
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
        engine
            .deny_tool("write-1", None, Some(&tx))
            .await
            .unwrap();
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
        assert!(turn_one[2].content_json.to_string().contains("子 Agent 完成"));
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
    async fn drain_pending_forks_injects_report_with_unique_sequences() {
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
                .fork_queue
                .lock()
                .expect("fork queue lock");
            guard.push((
                "KnowledgeAuditor".into(),
                "审计 chapters/chapter-001.md".into(),
            ));
        }

        engine.drain_pending_forks(None).await.unwrap();

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
        assert_eq!(engine.shared.sub_agent_count.load(std::sync::atomic::Ordering::SeqCst), 0);
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
                .fork_queue
                .lock()
                .expect("fork queue lock");
            guard.push((
                "KnowledgeAuditor".into(),
                "审计 chapters/chapter-001.md".into(),
            ));
        }

        engine.drain_pending_forks(None).await.unwrap();

        let stored = engine
            .shared
            .session
            .db
            .get_session_messages(&engine.shared.session.id, None)
            .unwrap();
        let report = stored
            .iter()
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
    async fn drain_pending_forks_injects_multiple_reports_in_order() {
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
                .fork_queue
                .lock()
                .expect("fork queue lock");
            guard.push((
                "KnowledgeAuditor".into(),
                "任务 A：chapter-001".into(),
            ));
            guard.push((
                "ChapterCraftAnalyzer".into(),
                "任务 B：chapter-001".into(),
            ));
        }

        engine.drain_pending_forks(None).await.unwrap();

        let stored = engine
            .shared
            .session
            .db
            .get_session_messages(&engine.shared.session.id, None)
            .unwrap();
        let reports: Vec<_> = stored
            .iter()
            .filter(|m| m.content_json.to_string().contains("子 Agent 完成"))
            .collect();
        assert_eq!(reports.len(), 2);
        assert_eq!(reports[0].sequence, 2);
        assert_eq!(reports[1].sequence, 3);
        assert!(
            reports[0]
                .content_json
                .to_string()
                .contains("KnowledgeAuditor")
        );
        assert!(
            reports[1]
                .content_json
                .to_string()
                .contains("ChapterCraftAnalyzer")
        );
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
