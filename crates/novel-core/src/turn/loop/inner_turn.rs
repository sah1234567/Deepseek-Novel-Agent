use crate::hooks::main_tool_schemas;
use crate::message::{assistant_from_completion, to_llm_messages_traced, RepairTraceContext};
use crate::subagent::{clear_subagent_queue, drain_subagent_jobs};
use crate::turn::llm_stream::LlmCallOutcome;
use crate::turn::todo_nudge::{
    no_tools_completion_action, unfinished_todo_nudge_message, NoToolsCompletionAction,
    REASONING_ONLY_NUDGE,
};
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
        let unfinished_count = self
            .list_session_todos()
            .iter()
            .filter(|t| t.is_unfinished())
            .count();
        let action = no_tools_completion_action(
            self.interrupt_requested(),
            self.pending_tools.len(),
            completion,
            unfinished_count,
            turn_ctx.todo_nudge_count,
        );
        match action {
            NoToolsCompletionAction::EarlyExit(reason) => {
                if !self.pending_tools.is_empty() {
                    tracing::debug!(
                        pending_tool_count = self.pending_tools.len(),
                        "inner_turn_paused_pending_tool_approval"
                    );
                }
                return Ok(Some(reason));
            }
            NoToolsCompletionAction::InjectReasoningNudge => {
                tracing::debug!(
                    inner_turn = turn_ctx.inner_turn,
                    reasoning_len = completion.reasoning_content.as_ref().map(|s| s.len()).unwrap_or(0),
                    stop_reason = ?completion.stop_reason,
                    "inner_turn_continue_reasoning_only"
                );
                self.inject_hidden_user_prompt(REASONING_ONLY_NUDGE)?;
                return match turn_ctx.increment_inner() {
                    Ok(()) => Ok(None),
                    Err(e) => Ok(Some(e)),
                };
            }
            NoToolsCompletionAction::InjectTodoNudge => {
                if self.maybe_inject_todo_nudge(turn_ctx)? {
                    return Ok(None);
                }
            }
            NoToolsCompletionAction::Complete => {}
        }
        tracing::debug!(
            inner_turn = turn_ctx.inner_turn,
            content_len = completion.content.as_ref().map(|s| s.len()).unwrap_or(0),
            reasoning_len = completion.reasoning_content.as_ref().map(|s| s.len()).unwrap_or(0),
            stop_reason = ?completion.stop_reason,
            "inner_turn_terminal_no_tools"
        );
        Ok(Some(TerminalReason::Completed))
    }

    fn inject_hidden_user_prompt(&mut self, prompt: &str) -> Result<(), AgentError> {
        let meta_msg = crate::ChatMessage {
            role: "user".into(),
            content: prompt.to_string(),
            display_content: Some(String::new()),
            ..Default::default()
        };
        self.persist_message_alloc(&meta_msg)?;
        self.messages.push(meta_msg);
        Ok(())
    }

    pub(crate) fn maybe_inject_todo_nudge(
        &mut self,
        turn_ctx: &mut TurnContext,
    ) -> Result<bool, AgentError> {
        const MAX_TODO_NUDGES: u32 = crate::turn::todo_nudge::MAX_TODO_NUDGES;
        // After MAX_TODO_NUDGES, allow turn to end (fail-open) to avoid infinite ReAct loops.
        if turn_ctx.todo_nudge_count >= MAX_TODO_NUDGES {
            return Ok(false);
        }
        let todos = self.list_session_todos();
        let unfinished: Vec<_> = todos
            .iter()
            .filter(|t| t.is_unfinished())
            .cloned()
            .collect();
        let Some(nudge) = unfinished_todo_nudge_message(&unfinished) else {
            return Ok(false);
        };
        turn_ctx.todo_nudge_count += 1;
        self.inject_hidden_user_prompt(&nudge)?;
        tracing::debug!(
            inner_turn = turn_ctx.inner_turn,
            unfinished_count = unfinished.len(),
            nudge_count = turn_ctx.todo_nudge_count,
            "inner_turn_todo_nudge_injected"
        );
        Ok(true)
    }

    fn inner_turn_loop_exit_reason(&mut self, turn_ctx: &TurnContext) -> Option<TerminalReason> {
        if self.interrupt_requested() {
            return Some(TerminalReason::AbortedStreaming);
        }
        if let Some((tool, detail)) = self.take_repeated_tool_failure_trip() {
            tracing::warn!(
                tool = %tool,
                detail = %detail,
                "inner_turn_circuit_breaker"
            );
            return Some(TerminalReason::RepeatedToolFailures { tool, detail });
        }
        if !self.pending_tools.is_empty() {
            return Some(TerminalReason::Completed);
        }
        if !turn_ctx.needs_continuation() {
            return Some(TerminalReason::MaxReactLoops(turn_ctx.max_inner_turns));
        }
        None
    }

    pub(in crate::turn::r#loop) async fn run_inner_turn_loop(
        &mut self,
        turn_ctx: &mut TurnContext,
        event_tx: Option<&mpsc::UnboundedSender<Event>>,
    ) -> Result<TerminalReason, AgentError> {
        loop {
            if let Some(reason) = self.inner_turn_loop_exit_reason(turn_ctx) {
                return Ok(reason);
            }

            let schemas = main_tool_schemas(&self.shared.registry);

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
            if let Some(reason) = self
                .process_inner_turn_llm_result(&completion, turn_ctx, event_tx)
                .await?
            {
                return Ok(reason);
            }
        }
    }

    async fn process_inner_turn_llm_result(
        &mut self,
        completion: &LlmCompletion,
        turn_ctx: &mut TurnContext,
        event_tx: Option<&mpsc::UnboundedSender<Event>>,
    ) -> Result<Option<TerminalReason>, AgentError> {
        if let Err(e) = crate::read_cache::sync::flush_dirty_read_cache_paths(
            &self.shared,
            self.turn_number as i32,
            self.turn_message_seq,
        ) {
            tracing::warn!(error = %e, "read_cache flush after inner LLM API failed");
        }

        if let Some(u) = &completion.usage {
            self.last_context_tokens =
                (u.cache_hit_tokens + u.cache_miss_tokens + u.completion_tokens) as usize;
        }
        if self.compaction_needed() {
            self.compact_with_events(event_tx).await;
        }

        if completion.tool_calls.is_empty() {
            return self
                .complete_inner_turn_without_tools(completion, turn_ctx)
                .await;
        }

        if self.llm.is_none() {
            let assistant = assistant_from_completion(completion);
            self.persist_message_alloc(&assistant)?;
            self.messages.push(assistant);
        }

        match turn_ctx.increment_inner() {
            Ok(()) => Ok(None),
            Err(reason) => Ok(Some(reason)),
        }
    }
}

#[cfg(test)]
mod inner_turn_tests {
    use super::*;
    use crate::turn::todo_nudge::REASONING_ONLY_NUDGE;
    use crate::{EngineConfig, TerminalReason};
    use novel_state::SessionTodo;
    use tempfile::TempDir;

    fn test_engine(tmp: &TempDir) -> AgentEngine {
        std::fs::create_dir_all(tmp.path().join("skills")).unwrap();
        AgentEngine::new(EngineConfig {
            project_root: tmp.path().to_path_buf(),
            settings_path: tmp.path().join("settings.json"),
            db_path: tmp.path().join("state.db"),
            skills_dir: tmp.path().join("skills"),
            global_config_path: tmp.path().join(".novel-agent/api_config.json"),
        })
        .unwrap()
    }

    #[tokio::test]
    async fn no_tools_text_completion_ends_turn() {
        let tmp = TempDir::new().unwrap();
        let mut engine = test_engine(&tmp);
        let mut turn_ctx = TurnContext::new(40);
        let completion = LlmCompletion {
            content: Some("final answer".into()),
            reasoning_content: None,
            tool_calls: vec![],
            stop_reason: Some("stop".into()),
            usage: None,
        };
        let out = engine
            .complete_inner_turn_without_tools(&completion, &mut turn_ctx)
            .await
            .unwrap();
        assert!(matches!(out, Some(TerminalReason::Completed)));
        assert!(engine.messages.iter().any(|m| m.content == "final answer"));
    }

    #[tokio::test]
    async fn no_tools_reasoning_only_injects_nudge_and_continues() {
        let tmp = TempDir::new().unwrap();
        let mut engine = test_engine(&tmp);
        let mut turn_ctx = TurnContext::new(40);
        let completion = LlmCompletion {
            content: None,
            reasoning_content: Some("thinking only".into()),
            tool_calls: vec![],
            stop_reason: Some("stop".into()),
            usage: None,
        };
        let out = engine
            .complete_inner_turn_without_tools(&completion, &mut turn_ctx)
            .await
            .unwrap();
        assert!(out.is_none());
        assert!(engine
            .messages
            .iter()
            .any(|m| m.role == "user" && m.content == REASONING_ONLY_NUDGE));
        assert_eq!(turn_ctx.inner_turn, 1);
    }

    #[tokio::test]
    async fn no_tools_unfinished_todos_inject_todo_nudge() {
        let tmp = TempDir::new().unwrap();
        let mut engine = test_engine(&tmp);
        engine
            .shared
            .session
            .db
            .upsert_session_todos(
                &engine.shared.session.id,
                &[SessionTodo {
                    id: "t1".into(),
                    content: "finish chapter".into(),
                    status: "pending".into(),
                }],
                true,
            )
            .unwrap();
        let mut turn_ctx = TurnContext::new(40);
        let completion = LlmCompletion {
            content: Some("done for now".into()),
            reasoning_content: None,
            tool_calls: vec![],
            stop_reason: Some("stop".into()),
            usage: None,
        };
        let out = engine
            .complete_inner_turn_without_tools(&completion, &mut turn_ctx)
            .await
            .unwrap();
        assert!(out.is_none());
        assert_eq!(turn_ctx.todo_nudge_count, 1);
        assert!(engine
            .messages
            .iter()
            .any(|m| m.role == "user" && m.content.contains("finish chapter")));
    }

    #[tokio::test]
    async fn no_tools_pending_tools_exits_early() {
        let tmp = TempDir::new().unwrap();
        let mut engine = test_engine(&tmp);
        engine.pending_tools.insert(
            "p1".into(),
            novel_tools::ToolCallSpec {
                id: "p1".into(),
                name: "Read".into(),
                input: serde_json::json!({}),
            },
        );
        let mut turn_ctx = TurnContext::new(40);
        let completion = LlmCompletion {
            content: Some("waiting".into()),
            reasoning_content: None,
            tool_calls: vec![],
            stop_reason: Some("stop".into()),
            usage: None,
        };
        let out = engine
            .complete_inner_turn_without_tools(&completion, &mut turn_ctx)
            .await
            .unwrap();
        assert!(matches!(out, Some(TerminalReason::Completed)));
    }
}
