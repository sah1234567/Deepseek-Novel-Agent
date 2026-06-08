use crate::hooks::tool_schemas_for_agent;
use crate::message::{assistant_from_completion, to_llm_messages_traced, RepairTraceContext};
use crate::subagent::{clear_subagent_queue, drain_subagent_jobs};
use crate::turn::llm_stream::{should_continue_inner_after_completion, LlmCallOutcome};
use crate::turn::TurnContext;
use crate::{AgentEngine, AgentError, Event, TerminalReason};
use novel_deepseek::LlmCompletion;
use tokio::sync::mpsc;

impl AgentEngine {
    /// Prefix for sub-agent reports injected mid-turn (role stays `user` for the LLM).
    pub(crate) const SUB_AGENT_REPORT_PREFIX: &'static str = crate::turn::SUB_AGENT_REPORT_PREFIX;

    pub(in crate::turn::r#loop) fn resume_inner_turn_from_messages(&self) -> u32 {
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
    pub(in crate::turn::r#loop) async fn run_inner_turn_loop(
        &mut self,
        turn_ctx: &mut TurnContext,
        event_tx: Option<&mpsc::UnboundedSender<Event>>,
    ) -> Result<TerminalReason, AgentError> {
        loop {
            if self.interrupt_requested() {
                return Ok(TerminalReason::AbortedStreaming);
            }
            if let Some((tool, detail)) = self.take_repeated_tool_failure_trip() {
                tracing::warn!(
                    tool = %tool,
                    detail = %detail,
                    "inner_turn_circuit_breaker"
                );
                return Ok(TerminalReason::RepeatedToolFailures { tool, detail });
            }
            if !self.pending_tools.is_empty() {
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
    fn main_tool_names(&self) -> Vec<String> {
        self.shared.registry.names()
    }
}
