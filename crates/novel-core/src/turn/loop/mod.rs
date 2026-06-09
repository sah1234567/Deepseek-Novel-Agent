mod compaction;
mod inner_turn;
mod persistence;
#[cfg(test)]
mod tests;

use crate::engine::session_llm::{
    build_chat_client, read_session_llm, write_session_llm, SessionLlmSnapshot,
};
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

        let author_content = content.trim().to_string();

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
        let merged_content = if let Some(prefix) = self.pending_permission_user_prefix.take() {
            tracing::debug!(
                prefix_len = prefix.len(),
                has_display_content = true,
                "permission_mode_prefix_prepended_to_user_message"
            );
            crate::permission::prepend_permission_notice(&prefix, &author_content)
        } else {
            author_content.clone()
        };
        let display_content = if crate::permission::is_permission_mode_notice(&merged_content) {
            Some(author_content.as_str())
        } else {
            None
        };
        let user_msg = ChatMessage {
            role: "user".into(),
            content: merged_content,
            tool_call_id: None,
            tool_calls: None,
            reasoning_content: None,
            display_content: display_content.map(str::to_string),
        };
        self.messages.push(user_msg.clone());
        self.turn_message_seq = 0;
        self.persist_message_at_seq(&user_msg, MSG_SEQ_USER, display_content)?;

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

        self.reset_tool_failure_circuit();
        let max_react = self.shared.settings.agent.max_react_loops;
        let mut turn_ctx = TurnContext::new(max_react);
        let reason = self.run_inner_turn_loop(&mut turn_ctx, event_tx).await?;

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
