//! Automatic context compaction: when the token budget is exceeded, summarizes older
//! messages into a compressed prefix, clears the read-file cache, and re-emits
//! fresh skill/agent summaries that survive the compaction boundary.

use std::sync::Arc;

use crate::context::dynamic_context::{
    filter_loadable_reference_paths, filter_loadable_skill_ids, format_activated_skill_block,
};
use crate::engine::session_llm::{apply_session_usage, read_session_llm};
use crate::interrupt::ERROR_MESSAGE_USER_ABORT;
use crate::message::{
    chat_slice_to_compaction, chat_to_compaction, compaction_slice_to_chat, to_llm_messages,
};
use crate::{AgentEngine, AgentError, CompactionAction, Event};
use novel_deepseek::{LlmChatMessage, LlmError};
use novel_logging::LogEvent;
use tokio::sync::mpsc;

impl AgentEngine {
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
        event_tx: Option<&mpsc::UnboundedSender<Event>>,
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
            Ok(completion) => {
                if let Some(u) = &completion.usage {
                    let snap = read_session_llm(&self.shared);
                    apply_session_usage(&self.shared, u, &snap, event_tx, false);
                }
                completion
                    .content
                    .filter(|c| !c.contains(ERROR_MESSAGE_USER_ABORT) && !c.trim().is_empty())
                    .map(|c| truncate_summary(&c, max_chars))
                    .unwrap_or_else(fallback)
            }
            Err(LlmError::Cancelled) => fallback(),
            Err(_) => fallback(),
        }
    }

    /// Compact context and replace session messages. Main agent only.
    const MAX_CONSECUTIVE_COMPACTION_FAILURES: u32 = 3;

    pub(in crate::turn::r#loop) async fn compact_and_sync(
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
                event_tx,
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

        let retained_bounds = {
            use crate::message::turn_rows::retained_turn_bounds_from_index;
            use novel_compaction::user_turn_ranges;

            let kept_turns = if final_msgs.len() > 2 {
                user_turn_ranges(&final_msgs[2..]).len()
            } else {
                0
            };
            if kept_turns == 0 {
                None
            } else {
                let ranges = user_turn_ranges(&compacted);
                let start_index = if ranges.len() >= kept_turns {
                    ranges[ranges.len() - kept_turns].0
                } else {
                    partition.retain_from
                };
                retained_turn_bounds_from_index(&self.messages, start_index)
            }
        };
        if let Some((min, max)) = retained_bounds {
            let _ = self.shared.session.db.record_compaction_retained_turns(
                &self.shared.session.id,
                epoch,
                min,
                max,
            );
        }

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
                epoch,
                retained_min_turn: retained_bounds.map(|(min, _)| min),
                retained_max_turn: retained_bounds.map(|(_, max)| max),
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
}
