use crate::dynamic_context::{
    filter_loadable_reference_paths, filter_loadable_skill_ids, format_activated_skill_block,
};
use crate::interrupt::ERROR_MESSAGE_USER_ABORT;
use crate::llm_stream_turn::should_continue_inner_after_completion;
#[allow(unused_imports)]
use crate::llm_stream_turn::{run_abort_bridge, LlmCallOutcome};
use crate::message_bridge::{
    assistant_from_completion, chat_slice_to_compaction, chat_to_compaction, chat_to_json,
    compaction_slice_to_chat, to_llm_messages, to_llm_messages_traced, tool_result_message,
    RepairTraceContext,
};
use crate::session_llm::{
    apply_session_usage, build_chat_client, read_session_llm, write_session_llm, SessionLlmSnapshot,
};
use crate::streaming_tool_dispatch::format_tool;
use crate::subagent::{clear_subagent_queue, drain_subagent_jobs};
use crate::turn::TurnContext;
use crate::turn::MSG_SEQ_USER;
use crate::{
    hooks::tool_schemas_for_agent, AgentEngine, AgentError, AgentType, ChatMessage,
    CompactionAction, Event, TerminalReason,
};
use novel_deepseek::{LlmChatMessage, LlmCompletion, LlmError};
use novel_logging::LogEvent;
use novel_tools::{ToolCallSpec, ToolExecutor};
use std::collections::HashSet;
use std::sync::Arc;
use tokio::sync::mpsc;

const MAIN_MAX_INNER_TURNS: u32 = 80;

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

    /// `None` means continue the inner ReAct loop (reasoning-only assistant segment).
    async fn complete_inner_turn_without_tools(
        &mut self,
        completion: &LlmCompletion,
        turn_ctx: &mut TurnContext,
    ) -> Result<Option<TerminalReason>, AgentError> {
        let assistant = assistant_from_completion(completion);
        self.persist_message_alloc(&assistant)?;
        self.messages.push(assistant);
        if self.interrupt_requested() {
            return Ok(Some(TerminalReason::AbortedStreaming));
        }
        if !self.pending_tools.is_empty() {
            tracing::debug!(
                pending_tool_count = self.pending_tools.len(),
                "inner_turn_paused_pending_tool_approval"
            );
            return Ok(Some(TerminalReason::Completed));
        }
        if should_continue_inner_after_completion(completion) {
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
                Ok(()) => return Ok(None),
                Err(e) => return Ok(Some(e)),
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
        Ok(Some(TerminalReason::Completed))
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
                if let Some(reason) = self
                    .complete_inner_turn_without_tools(&completion, turn_ctx)
                    .await?
                {
                    return Ok(reason);
                }
                continue;
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
} // end first impl AgentEngine block

impl AgentEngine {
    /// Returns true if turn should pause (e.g. AskUserQuestion).
    #[allow(clippy::too_many_arguments)]
    pub(crate) async fn execute_stream_results(
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

        let mut by_id = crate::tool_stream_results::merge_stream_results_by_id(results);
        let ordered_ids =
            crate::tool_stream_results::ordered_tool_result_ids(tool_call_order, &by_id);

        let mut pause_for_question = false;
        for id in ordered_ids {
            let result = match by_id.remove(&id) {
                Some(r) => r,
                None => continue,
            };
            let spec = spec_by_id.get(id.as_str()).copied();
            match result {
                Ok(out) => {
                    self.apply_ok_stream_result(&id, spec, out, event_tx, persist_tool_messages)?;
                }
                Err(novel_tools::ToolError::NeedsUserInput { payload }) => {
                    self.apply_needs_input_stream_result(
                        &id,
                        spec,
                        payload,
                        event_tx,
                        persist_tool_messages,
                    )?;
                    pause_for_question = true;
                }
                Err(e) => {
                    self.apply_err_stream_result(&id, spec, e, event_tx, persist_tool_messages)?;
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

        use novel_compaction::{
            partition_messages, rebuild_session_under_budget, SessionBudgetRebuildInput,
        };

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
        let compaction_threshold = self.shared.context_manager.threshold();

        let final_msgs = rebuild_session_under_budget(SessionBudgetRebuildInput {
            system,
            summary_text: &summary_text,
            retain: to_retain,
            skill_bodies: &skill_bodies,
            invoked_skill_ids: &skill_ids,
            retain_policy: &retain,
            window,
            compaction_threshold,
        })?;

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
            level: "session".into(),
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

    pub(crate) fn record_usage(
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
    use crate::streaming_tool_dispatch::StreamingToolDispatch;
    use crate::EngineConfig;
    use novel_deepseek::LlmToolCall;
    use novel_tools::PendingSubagentWork;
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
    async fn approve_unknown_tool_returns_validation() {
        let tmp = TempDir::new().unwrap();
        std::fs::create_dir_all(tmp.path().join("skills")).unwrap();
        let mut engine = AgentEngine::new(test_config(&tmp)).unwrap();
        let err = engine.approve_tool("missing-id", None).await.unwrap_err();
        assert!(matches!(err, AgentError::Validation(_)));
    }

    #[tokio::test]
    async fn approve_pending_read_persists_tool_message() {
        let _offline = crate::test_env::StripDeepseekApiKey::new();
        let tmp = TempDir::new().unwrap();
        std::fs::create_dir_all(tmp.path().join("skills")).unwrap();
        std::fs::write(tmp.path().join("notes.md"), "approved body").unwrap();
        let mut engine = AgentEngine::new(test_config(&tmp)).unwrap();
        engine.pending_tools.insert(
            "t-approve".into(),
            ToolCallSpec {
                id: "t-approve".into(),
                name: "Read".into(),
                input: serde_json::json!({"file_path": "notes.md"}),
            },
        );
        engine.approve_tool("t-approve", None).await.unwrap();
        assert!(engine.pending_tools.is_empty());
        assert!(engine.messages.iter().any(|m| {
            m.role == "tool"
                && m.tool_call_id.as_deref() == Some("t-approve")
                && m.content.contains("approved body")
        }));
    }

    #[tokio::test]
    async fn execute_stream_results_persists_tool_message() {
        let tmp = TempDir::new().unwrap();
        std::fs::create_dir_all(tmp.path().join("skills")).unwrap();
        let mut engine = AgentEngine::new(test_config(&tmp)).unwrap();
        let spec = ToolCallSpec {
            id: "t1".into(),
            name: "Read".into(),
            input: serde_json::json!({"file_path": "notes.md"}),
        };
        let pause = engine
            .execute_stream_results(
                vec![(
                    "t1".into(),
                    Ok(novel_tools::ToolOutput {
                        content: "file body".into(),
                        is_error: false,
                    }),
                )],
                std::slice::from_ref(&spec),
                &["t1".into()],
                &HashSet::new(),
                None,
                true,
            )
            .await
            .unwrap();
        assert!(!pause);
        assert!(engine
            .messages
            .iter()
            .any(|m| m.role == "tool" && m.tool_call_id.as_deref() == Some("t1")));
    }

    #[tokio::test]
    async fn execute_stream_results_pauses_on_needs_user_input() {
        let tmp = TempDir::new().unwrap();
        std::fs::create_dir_all(tmp.path().join("skills")).unwrap();
        let mut engine = AgentEngine::new(test_config(&tmp)).unwrap();
        let spec = ToolCallSpec {
            id: "q1".into(),
            name: "AskUserQuestion".into(),
            input: serde_json::json!({}),
        };
        let pause = engine
            .execute_stream_results(
                vec![(
                    "q1".into(),
                    Err(novel_tools::ToolError::NeedsUserInput {
                        payload: novel_tools::AskUserQuestionPayload { questions: vec![] },
                    }),
                )],
                std::slice::from_ref(&spec),
                &["q1".into()],
                &HashSet::new(),
                None,
                false,
            )
            .await
            .unwrap();
        assert!(pause);
        assert_eq!(engine.pending_user_question.as_deref(), Some("q1"));
    }

    #[tokio::test]
    async fn handle_ready_allows_read_tool() {
        use novel_tools::{abort_channel, default_registry, PermissionMode, ToolContext};
        let tmp = TempDir::new().unwrap();
        std::fs::write(tmp.path().join("x.md"), "body").unwrap();
        let registry = Arc::new(default_registry());
        let ctx = ToolContext {
            permission_mode: PermissionMode::Auto,
            project_root: tmp.path().to_path_buf(),
            ..ToolContext::new(tmp.path().to_path_buf())
        };
        let (_, abort_rx) = abort_channel();
        let mut dispatch = StreamingToolDispatch::new(registry.clone(), ctx.clone(), 4, abort_rx);
        dispatch.handle_ready(
            &registry,
            &ctx,
            None,
            LlmToolCall {
                id: "tc-read".into(),
                name: "Read".into(),
                arguments: r#"{"file_path":"x.md"}"#.into(),
            },
            true,
        );
        assert!(dispatch.handled_ids.contains("tc-read"));
        assert_eq!(dispatch.executed_specs.len(), 1);
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
