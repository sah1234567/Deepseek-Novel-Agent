use super::types::{open_audit_logger, AgentEngine, EngineConfig, EngineShared};
use crate::context::dynamic_context::{
    load_frozen_static_from_metadata, persist_frozen_system_metadata,
};
use crate::engine::session_llm::new_session_llm;
use crate::hooks::default_hook_config;
use crate::interrupt::AbortController;
use crate::message::{chat_to_json, repair_tool_use_chain, stored_to_chat};
use crate::{AgentError, ChatMessage, ContextManager, DynamicContext, SessionHandle};

use novel_config::{load_project_settings, ProjectSettings};
use novel_knowledge::KnowledgeStore;
use novel_tools::{default_registry, PermissionMode};

use std::collections::HashMap;
use std::sync::atomic::AtomicU32;
use std::sync::{Arc, Mutex};

use dashmap::DashMap;

impl AgentEngine {
    pub fn new(config: EngineConfig) -> Result<Self, AgentError> {
        Self::new_with_abort(config, AbortController::shared())
    }

    pub fn new_with_abort(
        config: EngineConfig,
        abort_controller: Arc<AbortController>,
    ) -> Result<Self, AgentError> {
        let mut settings = load_project_settings(&config.settings_path)?;
        if settings.hooks.post_tool_use.is_empty() {
            settings.hooks = default_hook_config();
        }

        let session = SessionHandle::create(
            config.project_root.clone(),
            config.db_path.clone(),
            &settings.model.model,
        )?;

        let registry = Arc::new(default_registry());
        let context_manager = ContextManager::new(&settings.model);
        let (system_prompt, agents_md, dynamic) =
            Self::build_initial_prompt(&config, &settings, &session)?;

        let messages = vec![ChatMessage {
            role: "system".into(),
            content: system_prompt.clone(),
            tool_call_id: None,
            tool_calls: None,
            reasoning_content: None,
        }];

        session
            .db
            .insert_message(
                &session.id,
                0,
                0,
                "system",
                &chat_to_json(&messages[0]),
                None,
            )
            .map_err(AgentError::from)?;
        persist_frozen_system_metadata(&session.db, &session.id, &dynamic)
            .map_err(AgentError::from)?;

        let initial_permission_mode = PermissionMode::from_settings_str(&settings.permissions.mode);
        session
            .db
            .set_session_permission_mode(&session.id, initial_permission_mode.label())
            .map_err(AgentError::from)?;

        let audit = open_audit_logger(&config.project_root, &session.id, &settings.model.model);

        let session_llm = new_session_llm(&settings);
        let shared = EngineShared {
            session,
            settings: Arc::new(settings),
            registry,
            context_manager,
            abort_controller,
            permission_mode_override: Arc::new(Mutex::new(initial_permission_mode)),
            read_file_cache: Arc::new(DashMap::new()),
            subagent_queue: Arc::new(Mutex::new(Vec::new())),
            session_llm,
            drain_in_progress: Arc::new(std::sync::atomic::AtomicBool::new(false)),
            agents_md,
            agent_skills_dir: config.skills_dir.clone(),
            global_config_path: config.global_config_path.clone(),
            system_prompt,
            sub_agent_count: Arc::new(AtomicU32::new(0)),
            compaction_lock: Arc::new(tokio::sync::Mutex::new(())),
            audit,
        };

        Ok(Self {
            shared,
            messages,
            is_streaming: false,
            turn_number: 0,
            pending_tools: HashMap::new(),
            pending_user_question: None,
            last_turn_usage: None,
            last_context_tokens: 0,
            has_interruptible_tool_in_progress: false,
            llm: None,
            invoked_skill_ids: Vec::new(),
            read_skill_reference_paths: Vec::new(),
            last_chapter_written: None,
            compaction_fail_count: 0,
            turn_message_seq: 0,
            pending_permission_user_prefix: None,
        })
    }

    pub fn resume(config: EngineConfig, session_id: &str) -> Result<Self, AgentError> {
        Self::resume_with_abort(config, session_id, AbortController::shared())
    }

    pub fn resume_with_abort(
        config: EngineConfig,
        session_id: &str,
        abort_controller: Arc<AbortController>,
    ) -> Result<Self, AgentError> {
        let mut settings = load_project_settings(&config.settings_path)?;
        if settings.hooks.post_tool_use.is_empty() {
            settings.hooks = default_hook_config();
        }

        let session = SessionHandle::resume(
            config.project_root.clone(),
            config.db_path.clone(),
            session_id,
        )?;

        let registry = Arc::new(default_registry());
        let context_manager = ContextManager::new(&settings.model);
        let stored = session.db.get_session_messages(session_id, None)?;

        let messages = if stored.is_empty() {
            let (system_prompt, agents_md, dynamic) =
                Self::build_initial_prompt(&config, &settings, &session)?;
            let msgs = vec![ChatMessage {
                role: "system".into(),
                content: system_prompt.clone(),
                tool_call_id: None,
                tool_calls: None,
                reasoning_content: None,
            }];
            session
                .db
                .insert_message(&session.id, 0, 0, "system", &chat_to_json(&msgs[0]), None)
                .map_err(AgentError::from)?;
            persist_frozen_system_metadata(&session.db, &session.id, &dynamic)
                .map_err(AgentError::from)?;
            let initial = PermissionMode::from_settings_str(&settings.permissions.mode);
            session
                .db
                .set_session_permission_mode(&session.id, initial.label())
                .map_err(AgentError::from)?;
            (msgs, system_prompt, agents_md)
        } else {
            session
                .db
                .require_frozen_system_metadata(session_id)
                .map_err(AgentError::from)?;
            let mut sp = stored_to_chat(&stored)?;
            repair_tool_use_chain(&mut sp);
            let sys = sp
                .first()
                .filter(|m| m.role == "system")
                .map(|m| m.content.clone())
                .ok_or_else(|| {
                    AgentError::Validation(
                        "session missing system message at (0,0); run scripts/reset-work-databases"
                            .into(),
                    )
                })?;
            let agents_md = load_frozen_static_from_metadata(&session.db, session_id)
                .map_err(AgentError::from)?
                .agents_md;
            (sp, sys, agents_md)
        };

        let (messages, system_prompt, agents_md) = messages;

        let turn_number = stored.iter().map(|m| m.turn_number).max().unwrap_or(0) as u32;
        let user_turn_count = stored
            .iter()
            .filter(|m| {
                m.role == "user"
                    && !m
                        .content_json
                        .get("content")
                        .and_then(|v| v.as_str())
                        .is_some_and(|c| c.starts_with("[上下文刷新]"))
            })
            .map(|m| m.turn_number)
            .max()
            .unwrap_or(0);
        if let Err(e) = session.db.sync_user_turn_count(session_id, user_turn_count) {
            tracing::warn!(
                session_id = %session_id,
                user_turn_count,
                error = %e,
                "sync_user_turn_count failed on resume"
            );
        }

        let invoked_skill_ids = session
            .db
            .get_invoked_skill_ids(session_id)
            .unwrap_or_default();

        let read_skill_reference_paths = session
            .db
            .get_read_skill_reference_paths(session_id)
            .unwrap_or_default();

        let initial_permission_mode = crate::permission::resolve_session_permission_mode(
            &session.db,
            session_id,
            &stored,
            &settings.permissions.mode,
        )
        .map_err(AgentError::from)?;
        tracing::debug!(
            mode = %initial_permission_mode.label(),
            session_id = %session_id,
            "resume_permission_mode"
        );

        let audit = open_audit_logger(&config.project_root, &session.id, &settings.model.model);

        let session_llm = new_session_llm(&settings);
        let shared = EngineShared {
            session,
            settings: Arc::new(settings),
            registry,
            context_manager,
            abort_controller,
            permission_mode_override: Arc::new(Mutex::new(initial_permission_mode)),
            read_file_cache: Arc::new(DashMap::new()),
            subagent_queue: Arc::new(Mutex::new(Vec::new())),
            session_llm,
            drain_in_progress: Arc::new(std::sync::atomic::AtomicBool::new(false)),
            agents_md,
            agent_skills_dir: config.skills_dir.clone(),
            global_config_path: config.global_config_path.clone(),
            system_prompt,
            sub_agent_count: Arc::new(AtomicU32::new(0)),
            compaction_lock: Arc::new(tokio::sync::Mutex::new(())),
            audit,
        };

        Ok(Self {
            shared,
            messages,
            is_streaming: false,
            turn_number,
            pending_tools: HashMap::new(),
            pending_user_question: None,
            last_turn_usage: None,
            last_context_tokens: 0,
            has_interruptible_tool_in_progress: false,
            llm: None,
            invoked_skill_ids,
            read_skill_reference_paths,
            last_chapter_written: None,
            compaction_fail_count: 0,
            turn_message_seq: 0,
            pending_permission_user_prefix: None,
        })
    }

    fn build_initial_prompt(
        config: &EngineConfig,
        settings: &ProjectSettings,
        session: &SessionHandle,
    ) -> Result<(String, String, DynamicContext), AgentError> {
        let store = KnowledgeStore::new(&config.project_root);
        let agents = store
            .read_file("AGENTS.md")
            .unwrap_or_else(|_| "默认：第三人称限知，2000-3000字/章".into());
        let (prompt, dynamic) =
            Self::assemble_system_prompt(config, session, &agents, &settings.permissions.mode)?;
        Ok((prompt, agents, dynamic))
    }
}
