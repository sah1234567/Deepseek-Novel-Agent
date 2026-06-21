use super::types::AgentEngine;
use crate::{AgentError, AgentType, Event, ForkedAgentContext, Op, TerminalReason};

use novel_tools::{PermissionMode, ToolContext};

use std::sync::atomic::Ordering;
use std::sync::Arc;
use tokio::sync::mpsc;

impl AgentEngine {
    pub async fn handle_message(&mut self, content: &str) -> Result<TerminalReason, AgentError> {
        self.handle_message_with_events(content, None, None).await
    }

    // ── Fork context builder (tests / direct API) ─────────────
    // Execution of subagents is only via `subagent_queue` → `drain_subagent_jobs` → `run_subagent_job`.

    pub async fn fork(
        &self,
        agent_type: AgentType,
        task_prompt: String,
    ) -> Result<ForkedAgentContext, AgentError> {
        tracing::debug!(
            session_id = %self.shared.session.id,
            ?agent_type,
            task_len = task_prompt.len(),
            "fork_agent"
        );
        if self.is_streaming {
            tracing::warn!("fork rejected: agent busy (streaming)");
            return Err(AgentError::AgentBusy);
        }
        if self.shared.sub_agent_count.load(Ordering::SeqCst) > 0 {
            return Err(AgentError::NestedForkProhibited);
        }

        crate::subagent::build_fork_child(&self.shared, agent_type, task_prompt)
    }

    // ── Tool context ──────────────────────────────────────────

    pub fn tool_context(&self) -> ToolContext {
        ToolContext {
            permission_mode: PermissionMode::from_settings_str(
                &self.shared.settings.permissions.mode,
            ),
            deny_rules: self.shared.settings.permissions.deny_rules.clone(),
            always_allow: crate::agent::merge_tool_always_allow(
                &self.shared.settings.permissions.always_allow,
            ),
            project_root: self.shared.session.project_root.clone(),
            session_id: self.shared.session.id.clone(),
            db: Some(Arc::new(self.shared.session.db.clone())),
            permission_mode_override: Some(Arc::clone(&self.shared.permission_mode_override)),
            read_file_cache: Some(Arc::clone(&self.shared.read_file_cache)),
            file_op_locks: Some(Arc::clone(&self.shared.file_op_locks)),
            allow_fork: self.shared.sub_agent_count.load(Ordering::SeqCst) == 0,
            subagent_queue: Some(Arc::clone(&self.shared.subagent_queue)),
            current_tool_call_id: None,
            skills_dir: Some(self.shared.agent_skills_dir.clone()),
            global_api_config_path: Some(self.shared.global_config_path.clone()),
            on_read_cache_path_touched: Some(crate::read_cache::sync::read_cache_touch_callback(
                &self.shared.read_cache_dirty_paths,
            )),
            memory_fork_mode: false,
        }
    }

    // ── Sub-agent management (used by turn/loop/inner_turn.rs) ──

    /// Increment the running sub-agent count (called before spawn).
    pub fn sub_agent_inc(&self) {
        self.shared.sub_agent_inc();
    }

    /// Decrement the running sub-agent count (called when a spawned task completes).
    pub fn sub_agent_dec(&self) {
        self.shared.sub_agent_dec();
    }

    // ── Run loop (CLI / non-Tauri) ────────────────────────────

    pub async fn run(
        mut self,
        mut op_rx: mpsc::UnboundedReceiver<Op>,
        event_tx: mpsc::UnboundedSender<Event>,
    ) -> Result<TerminalReason, AgentError> {
        while let Some(op) = op_rx.recv().await {
            if let Some(reason) = self.dispatch_run_op(op, &event_tx).await? {
                return Ok(reason);
            }
        }
        Ok(TerminalReason::Completed)
    }

    /// Handle one `Op` in the non-Tauri run loop. `Some` = exit the loop with that reason.
    async fn dispatch_run_op(
        &mut self,
        op: Op,
        event_tx: &mpsc::UnboundedSender<Event>,
    ) -> Result<Option<TerminalReason>, AgentError> {
        match op {
            Op::SendMessage { content, model } => {
                self.run_op_send_message(&content, model.as_deref(), event_tx)
                    .await
            }
            Op::Interrupt => Ok(Some(self.run_op_interrupt())),
            Op::ApproveTool { tool_call_id } => {
                self.run_op_approve_tool(&tool_call_id, event_tx).await
            }
            Op::DenyTool {
                tool_call_id,
                reason,
            } => self.run_op_deny_tool(&tool_call_id, reason, event_tx).await,
            Op::ResumeSession { session_id } => self.run_op_resume_session(&session_id),
        }
    }

    async fn run_op_send_message(
        &mut self,
        content: &str,
        model: Option<&str>,
        event_tx: &mpsc::UnboundedSender<Event>,
    ) -> Result<Option<TerminalReason>, AgentError> {
        let reason = self
            .handle_message_with_events(content, model, Some(event_tx))
            .await?;
        Ok((!matches!(reason, TerminalReason::Completed)).then_some(reason))
    }

    fn run_op_interrupt(&self) -> TerminalReason {
        self.shared
            .abort_controller
            .request(crate::InterruptReason::UserCancel);
        TerminalReason::AbortedStreaming
    }

    async fn run_op_approve_tool(
        &mut self,
        tool_call_id: &str,
        event_tx: &mpsc::UnboundedSender<Event>,
    ) -> Result<Option<TerminalReason>, AgentError> {
        self.approve_tool(tool_call_id, Some(event_tx)).await?;
        Ok(None)
    }

    async fn run_op_deny_tool(
        &mut self,
        tool_call_id: &str,
        reason: Option<String>,
        event_tx: &mpsc::UnboundedSender<Event>,
    ) -> Result<Option<TerminalReason>, AgentError> {
        self.deny_tool(tool_call_id, reason, Some(event_tx)).await?;
        Ok(None)
    }

    fn run_op_resume_session(
        &self,
        session_id: &str,
    ) -> Result<Option<TerminalReason>, AgentError> {
        if session_id != self.shared.session.id {
            return Err(AgentError::Validation(
                "resume session id mismatch in run loop".into(),
            ));
        }
        Ok(None)
    }
}
