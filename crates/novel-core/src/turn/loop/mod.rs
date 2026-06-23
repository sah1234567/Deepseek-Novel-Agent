mod compaction;
mod compaction_helpers;
mod inner_turn;
mod memory_prefetch_gate;
mod persistence;
#[cfg(test)]
mod tests;
mod turn_start;

use crate::engine::session_llm::{build_chat_client, read_session_llm, write_session_llm};
use crate::message::tool_result_message;
use crate::turn::TurnContext;
use crate::turn::MSG_SEQ_USER;
use crate::{AgentEngine, AgentError, AgentType, ChatMessage, Event, TerminalReason};
use novel_logging::LogEvent;
use novel_tools::{ToolCallSpec, ToolExecutor};
use std::sync::Arc;
use tokio::sync::mpsc;

impl AgentEngine {
    pub fn init_llm(&mut self) {
        if self.llm.is_some() {
            return;
        }
        let snap = read_session_llm(&self.shared);
        self.llm = build_chat_client(&snap, &self.shared.global_config_path);
        self.sync_session_llm_from_llm();
    }

    /// Initialize the memory selector (V4 Flash) client from global config.
    /// No-op if already set. Uses the same API key source as the main LLM.
    fn init_memory_selector(&mut self) {
        if self.memory_selector.is_some() {
            return;
        }
        self.memory_selector =
            novel_memory::create_selector_from_config(&self.shared.global_config_path);
    }

    fn memory_extraction_context_since(&self, cursor: usize) -> novel_memory::ExtractionContext {
        let tool_calls: Vec<(&str, &serde_json::Value)> = self
            .messages
            .iter()
            .skip(cursor)
            .filter_map(|msg| msg.tool_calls.as_ref())
            .flat_map(|tcs| tcs.iter())
            .map(|tc| (tc.name.as_str(), &tc.arguments))
            .collect();
        novel_memory::ExtractionContext {
            message_count: self.messages.len(),
            project_root: self.shared.session.project_root.clone(),
            main_agent_wrote_memory: novel_memory::has_memory_writes_since(
                tool_calls.iter().copied(),
            ),
            had_ask_user_question: tool_calls
                .iter()
                .any(|(name, _)| *name == "AskUserQuestion"),
        }
    }

    /// Fire-and-forget memory fork when the turn truly finished (not paused for Q&A or approval).
    fn maybe_spawn_memory_extraction(&mut self, reason: &TerminalReason) {
        if *reason != TerminalReason::Completed {
            return;
        }
        if self.pending_user_question.is_some() || !self.pending_tools.is_empty() {
            tracing::debug!(
                pending_question = self.pending_user_question.is_some(),
                pending_tool_count = self.pending_tools.len(),
                "memory_extraction_deferred_turn_paused"
            );
            return;
        }
        let cursor = self.memory_extractor.cursor();
        let extraction_ctx = self.memory_extraction_context_since(cursor);
        if let Some(prepared) = self
            .memory_extractor
            .try_prepare_extraction(&extraction_ctx)
        {
            self.sync_session_llm_from_llm();
            let llm_snap = crate::engine::session_llm::read_session_llm(&self.shared);
            let recent = self.messages[cursor..].to_vec();
            let all_messages = Arc::new(self.messages.clone());
            crate::subagent::spawn_memory_extraction(
                self.shared.clone(),
                Arc::clone(&self.memory_extractor),
                prepared,
                recent,
                llm_snap,
                all_messages,
            );
        }
    }

    // ── Main agent turn ───────────────────────────────────────

    pub async fn handle_message_with_events(
        &mut self,
        content: &str,
        model_override: Option<&str>,
        event_tx: Option<&mpsc::UnboundedSender<Event>>,
    ) -> Result<TerminalReason, AgentError> {
        let author_content =
            turn_start::validate_turn_start(content, self.pending_user_question.is_some())?;
        self.clear_interrupt();
        self.turn_number += 1;

        if let Err(e) = self
            .shared
            .session
            .db
            .sync_user_turn_count(&self.shared.session.id, self.turn_number as i32)
        {
            tracing::warn!(
                session_id = %self.shared.session.id,
                turn_number = self.turn_number,
                error = %e,
                "sync_user_turn_count failed"
            );
        }

        // Set session title from first user message (author text only, not injected prefix).
        if self.turn_number == 1 {
            let title: String = author_content.chars().take(50).collect();
            if let Err(e) = self
                .shared
                .session
                .db
                .set_session_title(&self.shared.session.id, &title)
            {
                tracing::warn!(
                    session_id = %self.shared.session.id,
                    error = %e,
                    "set_session_title failed"
                );
            }
        }
        let (turn_snap, per_turn_model_override) =
            turn_start::resolve_turn_llm_snapshot(model_override, &self.shared.settings);
        write_session_llm(&self.shared, turn_snap.clone());
        // Must rebuild when override changes: `init_llm` skips if `self.llm` is already set.
        if per_turn_model_override {
            self.llm = build_chat_client(&turn_snap, &self.shared.global_config_path);
            self.sync_session_llm_from_llm();
        }

        self.pending_tools.clear();
        let user_msg = turn_start::build_turn_user_message(
            &author_content,
            self.pending_permission_user_prefix.take(),
        );
        let display_content = user_msg.display_content.clone();
        self.messages.push(user_msg.clone());
        self.turn_message_seq = 0;
        self.persist_message_at_seq(&user_msg, MSG_SEQ_USER, display_content.as_deref())?;

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

        // ── Memory prefetch (async, parallel to LLM streaming) ──
        self.init_memory_selector();
        let gate = memory_prefetch_gate::evaluate_memory_prefetch_gate(
            &author_content,
            &self.shared.session.project_root,
            &self
                .messages
                .iter()
                .map(|m| m.content.as_str())
                .collect::<Vec<_>>(),
        );
        if gate.should_skip() {
            tracing::debug!(
                word_count = gate.word_count,
                surfaced_bytes = gate.surfaced_bytes,
                skip_short_prompt = gate.skip_short_prompt,
                skip_budget_exceeded = gate.skip_budget_exceeded,
                "memory_prefetch_skipped"
            );
            self.memory_prefetch = None;
        } else {
            let memory_dir = self.shared.session.project_root.join("memory");
            self.memory_prefetch = Some(novel_memory::MemoryPrefetch::start(
                self.memory_selector.clone(),
                author_content.clone(),
                memory_dir,
                gate.surfaced_paths,
            ));
        }

        self.reset_tool_failure_circuit();
        let max_react = self.shared.settings.agent.max_react_loops;
        let mut turn_ctx = TurnContext::new(max_react);
        let reason = self.run_inner_turn_loop(&mut turn_ctx, event_tx).await?;

        // ── Consume memory prefetch results ──
        if let Some(prefetch) = self.memory_prefetch.take() {
            let surfaced = prefetch.consume().await;
            if !surfaced.is_empty() {
                for memory in &surfaced {
                    let attachment = novel_memory::MemoryPrefetch::format_attachment(memory);
                    let meta_msg = ChatMessage {
                        role: "user".into(),
                        content: attachment,
                        display_content: Some(String::new()),
                        ..Default::default()
                    };
                    self.messages.push(meta_msg);
                }
                tracing::debug!(count = surfaced.len(), "memory_prefetch_injected");
            }
        }

        // ── Memory extraction: fire-and-forget background task ──
        self.maybe_spawn_memory_extraction(&reason);

        tracing::info!(turn = self.turn_number, ?reason, "turn_complete");
        tracing::debug!(
            session_id = %session_id,
            turn_number = self.turn_number,
            ?reason,
            "turn_complete_detail"
        );
        self.emit_turn_finished(&reason, event_tx);
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
            content: format!(
                "{} {agent_type}]\n{output}",
                AgentEngine::SUB_AGENT_REPORT_PREFIX
            ),
            tool_call_id: None,
            tool_calls: None,
            reasoning_content: None,
            ..Default::default()
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
            crate::subagent::fork_transcript::finish_fork_run(
                &self.shared.session.db,
                run_id,
                "complete",
                Some(&report_id),
            )?;
        }
        Ok(())
    }

    pub(crate) async fn execute_stream_results(
        &mut self,
        results: Vec<(
            String,
            Result<novel_tools::ToolOutput, novel_tools::ToolError>,
        )>,
        executed_specs: &[ToolCallSpec],
        tool_call_order: &[String],
        event_tx: Option<&mpsc::UnboundedSender<Event>>,
        persist_tool_messages: bool,
        skip_ui_result_events: &std::collections::HashSet<String>,
    ) -> Result<bool, AgentError> {
        let spec_by_id: std::collections::HashMap<&str, &ToolCallSpec> =
            executed_specs.iter().map(|s| (s.id.as_str(), s)).collect();

        let mut by_id = crate::turn::tool_merge::merge_stream_results_by_id(results);
        let ordered_ids = crate::turn::tool_merge::ordered_tool_result_ids(tool_call_order, &by_id);

        let mut pause_for_question = false;
        for id in ordered_ids {
            let result = match by_id.remove(&id) {
                Some(r) => r,
                None => continue,
            };
            let spec = spec_by_id.get(id.as_str()).copied();
            let skip_ui_emit = skip_ui_result_events.contains(&id);
            match result {
                Ok(out) => {
                    self.apply_ok_stream_result(
                        &id,
                        spec,
                        out,
                        event_tx,
                        persist_tool_messages,
                        skip_ui_emit,
                    )?;
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
                    self.apply_err_stream_result(
                        &id,
                        spec,
                        e,
                        event_tx,
                        persist_tool_messages,
                        skip_ui_emit,
                    )?;
                }
            }
        }
        Ok(pause_for_question)
    }

    // ── approve_tool / deny_tool ──────────────────────────────

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
        match result {
            Ok(out) => {
                self.apply_ok_stream_result(tool_call_id, Some(&spec), out, event_tx, true, false)?;
            }
            Err(err) => {
                self.apply_err_stream_result(
                    tool_call_id,
                    Some(&spec),
                    err,
                    event_tx,
                    true,
                    false,
                )?;
            }
        }
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
        let reason_str = reason.unwrap_or_else(|| "denied by user".into());
        self.apply_err_stream_result(
            tool_call_id,
            Some(&spec),
            novel_tools::ToolError::PermissionDenied(reason_str),
            event_tx,
            true,
            false,
        )?;
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
        let max_react = self.shared.settings.agent.max_react_loops;
        let mut turn_ctx = TurnContext::new(max_react);
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
        tracing::debug!(
            session_id = %self.shared.session.id,
            turn_number = self.turn_number,
            ?reason,
            resume_inner_turn = resume_inner,
            "turn_continue_complete"
        );
        if self.pending_user_question.is_some() || !self.pending_tools.is_empty() {
            tracing::debug!(
                pending_question = self.pending_user_question.is_some(),
                pending_tool_count = self.pending_tools.len(),
                "turn_continue_paused"
            );
        }
        self.maybe_spawn_memory_extraction(&reason);
        self.emit_turn_finished(&reason, event_tx);
        Ok(reason)
    }

    /// Audit + `TurnComplete` IPC when the turn is not paused for approval or AskUserQuestion.
    pub(in crate::turn::r#loop) fn emit_turn_finished(
        &mut self,
        reason: &TerminalReason,
        event_tx: Option<&mpsc::UnboundedSender<Event>>,
    ) {
        let (hit, miss, comp, _ctx) = self.session_token_summary();
        let (th, tm, tc) = self
            .last_turn_usage
            .as_ref()
            .map(|u| (u.cache_hit_tokens, u.cache_miss_tokens, u.completion_tokens))
            .unwrap_or((0, 0, 0));
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
    }
}
