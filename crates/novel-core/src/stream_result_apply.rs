//! Apply one streaming tool result to session state (extracted from `execute_stream_results`).

use crate::dynamic_context::parse_skill_reference_path;
use crate::hooks::knowledge_auditor_hook_task;
use crate::message_bridge::tool_result_message;
use crate::streaming_tool_dispatch::format_tool;
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
    ) -> Result<(), AgentError> {
        let success = !out.is_error;
        let formatted = format_tool(spec, Ok(out));
        let content = formatted.content;
        let hook_task = if self.shared.settings.hooks.post_tool_use.is_empty() {
            None
        } else {
            spec.and_then(|s| {
                knowledge_auditor_hook_task(
                    &self.shared.settings.hooks,
                    &s.name,
                    Some(&s.input),
                    &formatted.hook_preview,
                )
            })
        };

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
                tool_call_id: id.to_string(),
                content: content.clone(),
            });
        }
        let tool_msg = tool_result_message(id, &content);
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
            self.track_invoke_skill_after_tool(s);
            if s.name == "Read" && success {
                self.track_read_skill_reference(s);
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
        Ok(())
    }

    fn track_invoke_skill_after_tool(&mut self, spec: &ToolCallSpec) {
        if spec.name != "InvokeSkill" {
            return;
        }
        let Some(skill_id) = spec
            .input
            .get("skill_id")
            .or_else(|| spec.input.get("skillId"))
            .and_then(|v| v.as_str())
        else {
            return;
        };
        if self.invoked_skill_ids.iter().any(|id| id == skill_id) {
            return;
        }
        self.invoked_skill_ids.push(skill_id.to_string());
        let _ = self
            .shared
            .session
            .db
            .set_invoked_skill_ids(&self.shared.session.id, &self.invoked_skill_ids);
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
        let _ = self.shared.session.db.set_read_skill_reference_paths(
            &self.shared.session.id,
            &self.read_skill_reference_paths,
        );
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
        Ok(())
    }

    pub(crate) fn apply_err_stream_result(
        &mut self,
        id: &str,
        spec: Option<&ToolCallSpec>,
        err: ToolError,
        event_tx: Option<&mpsc::UnboundedSender<Event>>,
        persist_tool_messages: bool,
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
        let msg = format_tool(spec, Err(err)).content;
        if let Some(tx) = event_tx {
            let _ = tx.send(Event::ToolCallResult {
                tool_call_id: id.to_string(),
                content: msg.clone(),
            });
        }
        let tool_msg = tool_result_message(id, &msg);
        if persist_tool_messages {
            self.persist_message_alloc(&tool_msg)?;
        }
        self.messages.push(tool_msg);
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::EngineConfig;
    use tempfile::TempDir;

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
            )
            .unwrap();
        assert!(engine
            .messages
            .iter()
            .any(|m| m.role == "tool" && m.tool_call_id.as_deref() == Some("e1")),);
    }
}
