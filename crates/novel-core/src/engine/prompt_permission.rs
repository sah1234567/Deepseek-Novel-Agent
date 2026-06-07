use super::types::{AgentEngine, EngineConfig, EngineStatus};
use crate::context::dynamic_context::{
    build_dynamic_context, load_frozen_static_from_metadata, refresh_system_dynamic_context,
};
use crate::{AgentError, DynamicContext, SessionHandle, SystemPromptBuilder};
use std::sync::atomic::Ordering;

use novel_tools::PermissionMode;

enum PermissionPrefixAction {
    Set(String),
    Clear,
    NoChange,
}

fn permission_prefix_action(
    plan: &crate::permission::ModeTransitionPlan,
    old_mode: &PermissionMode,
    new_mode: &PermissionMode,
    system_has_autonomous: bool,
    pending_prefix: Option<&str>,
) -> PermissionPrefixAction {
    match plan.merged_prefix().map(str::to_string) {
        Some(prefix) => PermissionPrefixAction::Set(prefix),
        None if matches!(plan, crate::permission::ModeTransitionPlan::None)
            && matches!(new_mode, PermissionMode::Unattended)
            && !matches!(old_mode, PermissionMode::Unattended)
            && system_has_autonomous
            && pending_prefix
                .is_some_and(|p| p.starts_with(crate::permission::PERMISSION_MODE_EXIT_PREFIX)) =>
        {
            PermissionPrefixAction::Clear
        }
        None => PermissionPrefixAction::NoChange,
    }
}

impl AgentEngine {
    /// Build system prompt from fresh dynamic context (Progress, Memory, INDEX, Skills).
    /// `permission_mode`: settings `mode` string —"unattended" embeds autonomous rules in system
    /// at session boundary (new session / post-compaction). Mid-session toggles use user notices.
    pub fn assemble_system_prompt(
        config: &EngineConfig,
        session: &SessionHandle,
        agents_md: &str,
        permission_mode: &str,
    ) -> Result<(String, DynamicContext), AgentError> {
        let dynamic = build_dynamic_context(
            &config.project_root,
            &session.id,
            &session.db,
            agents_md,
            &config.skills_dir,
        );
        let is_unattended = permission_mode == "unattended";
        let prompt = SystemPromptBuilder::new().build(&dynamic, is_unattended);
        Ok((prompt, dynamic))
    }

    /// Refresh dynamic system sections (Index/Memory/Progress/Skills summaries) while keeping AGENTS + Workspace frozen.
    pub fn refresh_system_dynamic_sections(&mut self) -> Result<(), AgentError> {
        let frozen =
            load_frozen_static_from_metadata(&self.shared.session.db, &self.shared.session.id)
                .map_err(AgentError::from)?;
        let ctx = refresh_system_dynamic_context(
            &self.shared.session.project_root,
            &self.shared.session.id,
            &self.shared.session.db,
            &self.shared.agent_skills_dir,
            &frozen,
        );
        let is_unattended = self
            .shared
            .permission_mode_override
            .lock()
            .map(|g| matches!(*g, PermissionMode::Unattended))
            .unwrap_or(false);
        let prompt = SystemPromptBuilder::new().build(&ctx, is_unattended);
        self.shared.system_prompt = prompt.clone();
        if let Some(m0) = self.messages.first_mut() {
            if m0.role == "system" {
                m0.content = prompt;
            }
        }
        Ok(())
    }

    /// True when the current turn has not fully completed (paused for question or tool approval).
    pub fn is_turn_in_progress(&self) -> bool {
        self.pending_user_question.is_some() || !self.pending_tools.is_empty()
    }

    /// Snapshot for Tauri / frontend status bar.
    pub fn status_snapshot(&self) -> EngineStatus {
        let mode = self.tool_context().effective_permission_mode();
        EngineStatus {
            session_id: self.shared.session.id.clone(),
            permission_mode: mode.label().to_string(),
            hook_running: self.shared.drain_in_progress.load(Ordering::SeqCst),
            pending_user_question: self.pending_user_question.is_some(),
            turn_in_progress: self.is_turn_in_progress(),
            turn_number: self.turn_number,
            project_initialized: self.shared.session.project_root.join("AGENTS.md").is_file(),
            has_interruptible_tool_in_progress: self.has_interruptible_tool_in_progress,
        }
    }

    pub(crate) fn set_permission_mode_override(&self, mode: novel_tools::PermissionMode) {
        if let Ok(mut g) = self.shared.permission_mode_override.lock() {
            *g = mode;
        }
    }

    /// Update live permission mode. Mid-session switches inject user notices instead of mutating
    /// `messages[0]` so the system prefix stays stable for KV cache.
    pub fn apply_permission_mode_change(
        &mut self,
        new_mode: PermissionMode,
    ) -> Result<(), AgentError> {
        let old_mode = self
            .shared
            .permission_mode_override
            .lock()
            .map(|g| g.clone())
            .unwrap_or(PermissionMode::Normal);
        if old_mode == new_mode {
            return Ok(());
        }

        if self.is_turn_in_progress() {
            tracing::warn!(
                pending_question = self.pending_user_question.is_some(),
                pending_tool_count = self.pending_tools.len(),
                "permission_mode_change_rejected_turn_in_progress"
            );
            return Err(AgentError::Validation(
                "cannot change permission mode while turn in progress".into(),
            ));
        }

        self.set_permission_mode_override(new_mode.clone());

        let system_has_autonomous = self
            .messages
            .first()
            .filter(|m| m.role == "system")
            .is_some_and(|m| crate::permission::system_contains_autonomous(&m.content));

        let plan =
            crate::permission::plan_mode_transition(&old_mode, &new_mode, system_has_autonomous);

        match permission_prefix_action(
            &plan,
            &old_mode,
            &new_mode,
            system_has_autonomous,
            self.pending_permission_user_prefix.as_deref(),
        ) {
            PermissionPrefixAction::Set(prefix) => {
                self.pending_permission_user_prefix = Some(prefix);
            }
            PermissionPrefixAction::Clear => self.pending_permission_user_prefix = None,
            PermissionPrefixAction::NoChange => {}
        }

        self.shared
            .session
            .db
            .set_session_permission_mode(&self.shared.session.id, new_mode.label())
            .map_err(AgentError::from)?;

        tracing::debug!(
            ?old_mode,
            ?new_mode,
            ?plan,
            system_has_autonomous,
            persisted = true,
            "permission_mode_changed"
        );
        Ok(())
    }
}
