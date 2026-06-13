//! Apply one streaming tool result to session state (extracted from `execute_stream_results`).

use crate::context::dynamic_context::parse_skill_reference_path;
use crate::hooks::knowledge_auditor_hook_task;
use crate::message::tool_result_message;
use crate::session_todos::maybe_emit_session_todos_after_tool;
use crate::turn::format_tool;
use crate::{AgentEngine, AgentError, Event};
use novel_logging::LogEvent;
use novel_tools::{PendingSubagentWork, ToolCallSpec, ToolError, ToolOutput};
use tokio::sync::mpsc;

impl AgentEngine {
    pub(crate) fn apply_ok_stream_result(
        &mut self,
        id: &str,
        spec: Option<&ToolCallSpec>,
        out: ToolOutput,
        event_tx: Option<&mpsc::UnboundedSender<Event>>,
        persist_tool_messages: bool,
        skip_ui_result_emit: bool,
    ) -> Result<(), AgentError> {
        let success = !out.is_error;
        let formatted = format_tool(&self.shared.registry, spec, Ok(out));
        let hook_task = self.post_tool_hook_task(spec, &formatted.hook_preview);
        let content = formatted.content;
        self.maybe_track_chapter_written(spec);
        self.emit_tool_result_ui(id, &content, event_tx, skip_ui_result_emit);
        let tool_msg = tool_result_message(id, &content);
        if persist_tool_messages {
            self.persist_message_alloc(&tool_msg)?;
        }
        self.audit_tool_success(id, spec, success);
        self.messages.push(tool_msg);
        self.record_tool_success();
        if let Some(s) = spec {
            self.apply_post_success_tool_effects(s, success, hook_task);
            // Streaming path already emitted via poll_ui_results (skip_ui_result_emit).
            if success && !skip_ui_result_emit {
                maybe_emit_session_todos_after_tool(
                    &s.name,
                    &self.shared.session.id,
                    &self.shared.session.db,
                    event_tx,
                );
            }
        }
        Ok(())
    }

    fn post_tool_hook_task(
        &self,
        spec: Option<&ToolCallSpec>,
        hook_preview: &str,
    ) -> Option<String> {
        if self.shared.settings.hooks.post_tool_use.is_empty() {
            return None;
        }
        spec.and_then(|s| {
            knowledge_auditor_hook_task(
                &self.shared.settings.hooks,
                &s.name,
                Some(&s.input),
                hook_preview,
            )
        })
    }

    fn maybe_track_chapter_written(&mut self, spec: Option<&ToolCallSpec>) {
        let Some(s) = spec else {
            return;
        };
        let Some(path) = novel_tools::optional_file_path(&s.input) else {
            return;
        };
        if novel_tools::normalize_rel_path(&path).contains("chapters/") {
            self.last_chapter_written = Some(novel_tools::normalize_chapter_progress_path(&path));
        }
    }

    fn emit_tool_result_ui(
        &self,
        id: &str,
        content: &str,
        event_tx: Option<&mpsc::UnboundedSender<Event>>,
        skip_ui_result_emit: bool,
    ) {
        if skip_ui_result_emit {
            return;
        }
        if let Some(tx) = event_tx {
            let _ = tx.send(Event::ToolCallResult {
                tool_call_id: id.to_string(),
                content: content.to_string(),
            });
        }
    }

    fn audit_tool_success(&self, id: &str, spec: Option<&ToolCallSpec>, success: bool) {
        let Some(s) = spec else {
            return;
        };
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

    fn apply_post_success_tool_effects(
        &mut self,
        spec: &ToolCallSpec,
        success: bool,
        hook_task: Option<String>,
    ) {
        if success {
            self.tool_context().promote_read_cache_for_tool_result(
                &self.shared.registry,
                &spec.name,
                &spec.input,
            );
        }
        let (track_skill, track_read_ref) = self
            .shared
            .registry
            .get(&spec.name)
            .map(|tool| (tool.is_skill_invocation(), tool.tracks_skill_references()))
            .unwrap_or((false, false));
        if track_skill {
            self.track_invoke_skill_after_tool(spec);
        }
        if success && track_read_ref {
            self.track_read_skill_reference(spec);
        }
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

    fn track_invoke_skill_after_tool(&mut self, spec: &ToolCallSpec) {
        let Some(skill_id) = spec.input.get("skill_id").and_then(|v| v.as_str()) else {
            return;
        };
        if self.invoked_skill_ids.iter().any(|id| id == skill_id) {
            return;
        }
        self.invoked_skill_ids.push(skill_id.to_string());
        if let Err(e) = self
            .shared
            .session
            .db
            .set_invoked_skill_ids(&self.shared.session.id, &self.invoked_skill_ids)
        {
            tracing::warn!(
                session_id = %self.shared.session.id,
                error = %e,
                skill_id,
                "set_invoked_skill_ids failed"
            );
        }
    }

    fn track_read_skill_reference(&mut self, spec: &ToolCallSpec) {
        let Some(path) = novel_tools::optional_file_path(&spec.input) else {
            return;
        };
        let Some((_, canonical)) = parse_skill_reference_path(
            &self.shared.session.project_root,
            &self.shared.agent_skills_dir,
            &path,
        ) else {
            return;
        };
        if self
            .read_skill_reference_paths
            .iter()
            .any(|p| p == &canonical)
        {
            return;
        }
        self.read_skill_reference_paths.push(canonical);
        if let Err(e) = self.shared.session.db.set_read_skill_reference_paths(
            &self.shared.session.id,
            &self.read_skill_reference_paths,
        ) {
            tracing::warn!(
                session_id = %self.shared.session.id,
                error = %e,
                "set_read_skill_reference_paths failed"
            );
        }
    }

    pub(crate) fn apply_needs_input_stream_result(
        &mut self,
        id: &str,
        spec: Option<&ToolCallSpec>,
        payload: novel_tools::AskUserQuestionPayload,
        event_tx: Option<&mpsc::UnboundedSender<Event>>,
        persist_tool_messages: bool,
    ) -> Result<(), AgentError> {
        tracing::debug!(tool_call_id = %id, "tool_needs_user_input");
        self.pending_user_question = Some(id.to_string());
        if let Some(s) = spec {
            self.audit_log(LogEvent::ToolExecuted {
                session_id: self.shared.session.id.clone(),
                tool_name: s.name.clone(),
                success: true,
            });
        }
        if let Some(tx) = event_tx {
            let _ = tx.send(Event::AskUserQuestion {
                tool_call_id: id.to_string(),
                payload,
            });
        }
        let tool_msg = tool_result_message(id, novel_tools::NEEDS_USER_INPUT_STUB);
        if persist_tool_messages {
            self.persist_message_alloc(&tool_msg)?;
        }
        self.messages.push(tool_msg);
        self.record_tool_success();
        Ok(())
    }

    pub(crate) fn apply_err_stream_result(
        &mut self,
        id: &str,
        spec: Option<&ToolCallSpec>,
        err: ToolError,
        event_tx: Option<&mpsc::UnboundedSender<Event>>,
        persist_tool_messages: bool,
        skip_ui_result_emit: bool,
    ) -> Result<(), AgentError> {
        if let Some(s) = spec {
            tracing::warn!(
                tool_call_id = %id,
                tool_name = %s.name,
                error = %err,
                "tool_executed_failed"
            );
            self.audit_log(LogEvent::ToolExecuted {
                session_id: self.shared.session.id.clone(),
                tool_name: s.name.clone(),
                success: false,
            });
        }
        let failure_detail = err.to_string();
        let msg = format_tool(&self.shared.registry, spec, Err(err)).content;
        if !skip_ui_result_emit {
            if let Some(tx) = event_tx {
                let _ = tx.send(Event::ToolCallResult {
                    tool_call_id: id.to_string(),
                    content: msg.clone(),
                });
            }
        }
        let tool_msg = tool_result_message(id, &msg);
        if persist_tool_messages {
            self.persist_message_alloc(&tool_msg)?;
        }
        self.messages.push(tool_msg);
        if let Some(s) = spec {
            self.record_tool_failure(&s.name, &failure_detail);
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::EngineConfig;
    use tempfile::TempDir;

    #[test]
    fn repeated_tool_failures_trip_circuit() {
        use crate::turn::TOOL_FAILURE_CIRCUIT_THRESHOLD;
        let tmp = TempDir::new().unwrap();
        std::fs::create_dir_all(tmp.path().join("skills")).unwrap();
        let mut engine = AgentEngine::new(EngineConfig {
            project_root: tmp.path().to_path_buf(),
            settings_path: tmp.path().join("settings.json"),
            db_path: tmp.path().join("state.db"),
            skills_dir: tmp.path().join("skills"),
            global_config_path: tmp.path().join(".novel-agent/api_config.json"),
        })
        .unwrap();
        let spec = novel_tools::ToolCallSpec {
            id: "t1".into(),
            name: "Edit".into(),
            input: serde_json::json!({}),
        };
        for _ in 0..TOOL_FAILURE_CIRCUIT_THRESHOLD {
            engine
                .apply_err_stream_result(
                    "t1",
                    Some(&spec),
                    novel_tools::ToolError::Execution("slice".into()),
                    None,
                    false,
                    false,
                )
                .unwrap();
        }
        assert!(engine.take_repeated_tool_failure_trip().is_some());
    }

    #[test]
    fn apply_ok_stream_result_pushes_tool_message() {
        let tmp = TempDir::new().unwrap();
        std::fs::create_dir_all(tmp.path().join("skills")).unwrap();
        std::fs::write(tmp.path().join("note.txt"), "hello").unwrap();
        let mut engine = AgentEngine::new(EngineConfig {
            project_root: tmp.path().to_path_buf(),
            settings_path: tmp.path().join("settings.json"),
            db_path: tmp.path().join("state.db"),
            skills_dir: tmp.path().join("skills"),
            global_config_path: tmp.path().join(".novel-agent/api_config.json"),
        })
        .unwrap();
        let spec = ToolCallSpec {
            id: "ok1".into(),
            name: "Read".into(),
            input: serde_json::json!({"file_path": "note.txt"}),
        };
        engine
            .apply_ok_stream_result(
                "ok1",
                Some(&spec),
                ToolOutput {
                    content: "1\thello".into(),
                    is_error: false,
                },
                None,
                false,
                false,
            )
            .unwrap();
        assert!(engine
            .messages
            .iter()
            .any(|m| m.role == "tool" && m.content.contains("hello")));
    }

    #[test]
    fn apply_err_stream_result_pushes_tool_message() {
        let tmp = TempDir::new().unwrap();
        std::fs::create_dir_all(tmp.path().join("skills")).unwrap();
        let mut engine = AgentEngine::new(EngineConfig {
            project_root: tmp.path().to_path_buf(),
            settings_path: tmp.path().join("settings.json"),
            db_path: tmp.path().join("state.db"),
            skills_dir: tmp.path().join("skills"),
            global_config_path: tmp.path().join(".novel-agent/api_config.json"),
        })
        .unwrap();
        let spec = ToolCallSpec {
            id: "e1".into(),
            name: "Read".into(),
            input: serde_json::json!({"file_path": "missing.txt"}),
        };
        engine
            .apply_err_stream_result(
                "e1",
                Some(&spec),
                ToolError::Execution("not found".into()),
                None,
                false,
                false,
            )
            .unwrap();
        assert!(engine
            .messages
            .iter()
            .any(|m| m.role == "tool" && m.tool_call_id.as_deref() == Some("e1")),);
    }
}
