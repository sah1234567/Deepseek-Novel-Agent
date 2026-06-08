use crate::engine::session_llm::{apply_session_usage, read_session_llm};
use crate::message::chat_to_json;
use crate::turn::TOOL_FAILURE_CIRCUIT_THRESHOLD;
use crate::Event;
use crate::{AgentEngine, AgentError, ChatMessage};
use novel_deepseek::LlmCompletion;
use tokio::sync::mpsc;

impl AgentEngine {
    pub(in crate::turn::r#loop) fn sync_messages_to_db(&self) -> Result<(), AgentError> {
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

    pub(in crate::turn::r#loop) fn build_message_rows(
        &self,
    ) -> Vec<(i32, i32, String, serde_json::Value)> {
        let mut rows = Vec::with_capacity(self.messages.len());
        let mut turn = 0i32;
        let mut seq_in_turn = 0i32;
        for msg in self.messages.iter() {
            let (t, seq) = crate::message::turn_rows::assign_message_turn_seq(
                msg,
                &mut turn,
                &mut seq_in_turn,
            );
            rows.push((t, seq, msg.role.clone(), chat_to_json(msg)));
        }
        rows
    }

    // ── Persistence helpers ────────────────────────────────────

    fn alloc_turn_message_seq(&mut self) -> i32 {
        self.turn_message_seq += 1;
        self.turn_message_seq
    }

    pub(in crate::turn::r#loop) fn persist_message_at_seq(
        &mut self,
        msg: &ChatMessage,
        sequence: i32,
        display_content: Option<&str>,
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
            has_display_content = display_content.is_some(),
            "persist_message"
        );
        let json = crate::message::chat_to_json_for_persist(msg, display_content);
        if let Err(e) = self.shared.session.db.insert_message(
            &self.shared.session.id,
            self.turn_number as i32,
            sequence,
            &msg.role,
            &json,
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

    pub(in crate::turn::r#loop) fn init_turn_message_seq_from_db(
        &mut self,
    ) -> Result<(), AgentError> {
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
    pub(in crate::turn::r#loop) fn persist_message_alloc_ex(
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

    pub(in crate::turn::r#loop) fn reset_tool_failure_circuit(&mut self) {
        self.consecutive_tool_failure_key = None;
        self.consecutive_tool_failure_count = 0;
    }

    pub(crate) fn record_tool_success(&mut self) {
        self.reset_tool_failure_circuit();
    }

    pub(crate) fn record_tool_failure(&mut self, tool_name: &str, detail: &str) {
        let key = format!("{tool_name}\x1f{detail}");
        if self.consecutive_tool_failure_key.as_deref() == Some(key.as_str()) {
            self.consecutive_tool_failure_count += 1;
        } else {
            self.consecutive_tool_failure_key = Some(key);
            self.consecutive_tool_failure_count = 1;
        }
    }

    pub(crate) fn take_repeated_tool_failure_trip(&mut self) -> Option<(String, String)> {
        if self.consecutive_tool_failure_count < TOOL_FAILURE_CIRCUIT_THRESHOLD {
            return None;
        }
        let key = self.consecutive_tool_failure_key.take()?;
        self.consecutive_tool_failure_count = 0;
        let (tool, detail) = key.split_once('\x1f')?;
        Some((tool.to_string(), detail.to_string()))
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
            apply_session_usage(&self.shared, u, &snap, event_tx, true);
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
}
