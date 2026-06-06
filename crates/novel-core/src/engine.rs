use crate::{
    AgentError, AgentType, ChatMessage, ContextManager, DynamicContext, Event, ForkedAgentContext,
    Op, SessionHandle, SystemPromptBuilder, TerminalReason,
};

use crate::dynamic_context::{
    build_dynamic_context, load_frozen_static_from_metadata, persist_frozen_system_metadata,
    refresh_system_dynamic_context,
};
use crate::hooks::default_hook_config;
use crate::interrupt::AbortController;
use crate::message_bridge::{chat_to_json, repair_tool_use_chain, stored_to_chat};

use novel_config::{load_project_settings, ProjectSettings};

use novel_knowledge::KnowledgeStore;

use novel_deepseek::ChatClient;
use novel_logging::{AuditLogger, LogEvent};

use crate::session_llm::{new_session_llm, write_session_llm, SessionLlm, SessionLlmSnapshot};
use novel_tools::{
    default_registry, PermissionMode, ReadCacheEntry, SubagentWorkQueue, ToolCallSpec, ToolContext,
    ToolRegistry,
};

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::{Arc, Mutex};

use dashmap::DashMap;

use tokio::sync::mpsc;

use serde::Serialize;

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct EngineStatus {
    pub session_id: String,
    pub permission_mode: String,
    /// True while `drain_subagent_jobs` is active (PostToolUse hook batch and/or tool forks).
    pub hook_running: bool,
    pub pending_user_question: bool,
    pub turn_number: u32,
    pub project_initialized: bool,
    pub has_interruptible_tool_in_progress: bool,
}

pub(crate) fn permission_mode_label(mode: &PermissionMode) -> &'static str {
    match mode {
        PermissionMode::Normal => "normal",
        PermissionMode::Plan => "plan",
        PermissionMode::Auto => "auto",
        PermissionMode::Unattended => "unattended",
    }
}

pub(crate) fn permission_mode_from_str(mode: &str) -> PermissionMode {
    match mode {
        "plan" => PermissionMode::Plan,
        "auto" => PermissionMode::Auto,
        "unattended" => PermissionMode::Unattended,
        _ => PermissionMode::Normal,
    }
}

#[derive(Clone)]
pub struct EngineConfig {
    pub project_root: PathBuf,
    pub settings_path: PathBuf,
    pub db_path: PathBuf,
    pub skills_dir: PathBuf,
    pub global_config_path: PathBuf,
}

// ── Shared state (clonable, sendable) ────────────────────────────

/// State shared between the main agent and async sub-agent tasks.
/// All fields are either `Clone` or wrapped in `Arc`.
#[derive(Clone)]
pub struct EngineShared {
    pub session: SessionHandle,
    pub settings: Arc<ProjectSettings>,
    pub registry: Arc<ToolRegistry>,
    pub context_manager: ContextManager,
    pub abort_controller: Arc<AbortController>,
    pub permission_mode_override: Arc<Mutex<PermissionMode>>,
    pub read_file_cache: Arc<DashMap<PathBuf, ReadCacheEntry>>,
    pub subagent_queue: SubagentWorkQueue,
    pub session_llm: SessionLlm,
    pub drain_in_progress: Arc<std::sync::atomic::AtomicBool>,
    pub agents_md: String,
    pub agent_skills_dir: PathBuf,
    pub global_config_path: PathBuf,
    pub system_prompt: String,
    pub sub_agent_count: Arc<AtomicU32>,

    /// Ensures only one compaction runs at a time on the main session.
    pub compaction_lock: Arc<tokio::sync::Mutex<()>>,
    /// Per-session JSONL audit log under `{project_root}/.novel/logs/session_{id}/`.
    pub audit: Option<Arc<AuditLogger>>,
}

impl EngineShared {
    pub fn audit_log(&self, event: &LogEvent) {
        if let Some(ref audit) = self.audit {
            if let Err(e) = audit.log(event) {
                tracing::warn!(error = %e, "audit log write failed");
            }
        }
    }

    /// Drop all session read-cache entries (e.g. after context compaction).
    pub fn clear_read_file_cache(&self) {
        self.read_file_cache.clear();
    }
}

pub fn open_audit_logger(
    project_root: &std::path::Path,
    session_id: &str,
    model: &str,
) -> Option<Arc<AuditLogger>> {
    match AuditLogger::open(project_root, session_id) {
        Ok(logger) => {
            let event = LogEvent::SessionCreated {
                session_id: session_id.to_string(),
                project_root: project_root.display().to_string(),
                model: model.to_string(),
            };
            if let Err(e) = logger.log(&event) {
                tracing::warn!(error = %e, "audit SessionCreated failed");
            }
            Some(Arc::new(logger))
        }
        Err(e) => {
            tracing::warn!(error = %e, "failed to open audit logger");
            None
        }
    }
}

// ── Main engine ──────────────────────────────────────────────────

pub struct AgentEngine {
    pub shared: EngineShared,

    pub messages: Vec<ChatMessage>,
    pub is_streaming: bool,
    pub turn_number: u32,
    pub(crate) pending_tools: HashMap<String, ToolCallSpec>,
    pub(crate) pending_user_question: Option<String>,
    pub(crate) last_turn_usage: Option<novel_deepseek::TokenUsage>,
    pub(crate) last_context_tokens: usize,
    pub(crate) has_interruptible_tool_in_progress: bool,
    /// Main-session LLM client; built via `session_llm::build_chat_client` / `init_llm`.
    pub(crate) llm: Option<ChatClient>,
    pub(crate) invoked_skill_ids: Vec<String>,
    pub(crate) read_skill_reference_paths: Vec<String>,
    pub(crate) last_chapter_written: Option<String>,
    pub(crate) compaction_fail_count: u32,
    /// Monotonic `(turn_number, sequence)` counter for the active user turn.
    /// User message is `0`; assistant/tool messages use `1, 2, 3…` in chat order.
    pub(crate) turn_message_seq: i32,
}

impl AgentEngine {
    /// Copy `self.llm` model/thinking into `EngineShared.session_llm` for subagent drain.
    pub(crate) fn sync_session_llm_from_llm(&self) {
        if let Some(ref client) = self.llm {
            write_session_llm(
                &self.shared,
                SessionLlmSnapshot {
                    model: client.model.clone(),
                    thinking_enabled: client.thinking_enabled,
                },
            );
        }
    }
}

impl AgentEngine {
    pub(crate) fn interrupt_requested(&self) -> bool {
        self.shared.abort_controller.is_aborted()
    }

    pub fn abort_reason(&self) -> Option<crate::InterruptReason> {
        self.shared.abort_controller.reason()
    }

    pub fn clear_interrupt(&self) {
        self.shared.abort_controller.clear();
    }

    pub(crate) fn audit_log(&self, event: LogEvent) {
        self.shared.audit_log(&event);
    }

    pub(crate) fn audit_error(&self, message: impl Into<String>, recoverable: bool) {
        self.audit_log(LogEvent::Error {
            message: message.into(),
            recoverable,
        });
    }
}

impl AgentEngine {
    pub fn new(config: EngineConfig) -> Result<Self, AgentError> {
        Self::new_with_abort(config, AbortController::shared())
    }

    pub fn new_with_abort(
        config: EngineConfig,
        abort_controller: Arc<AbortController>,
    ) -> Result<Self, AgentError> {
        let mut settings = load_project_settings(&config.settings_path).unwrap_or_default();
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

        let audit = open_audit_logger(&config.project_root, &session.id, &settings.model.model);

        let initial_permission_mode = permission_mode_from_str(&settings.permissions.mode);

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
        let mut settings = load_project_settings(&config.settings_path).unwrap_or_default();
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
            (msgs, system_prompt, agents_md)
        } else {
            session
                .db
                .require_frozen_system_metadata(session_id)
                .map_err(AgentError::from)?;
            let mut sp = stored_to_chat(&stored);
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
        let _ = session.db.sync_user_turn_count(session_id, user_turn_count);

        let invoked_skill_ids = session
            .db
            .get_invoked_skill_ids(session_id)
            .unwrap_or_default();

        let read_skill_reference_paths = session
            .db
            .get_read_skill_reference_paths(session_id)
            .unwrap_or_default();

        let audit = open_audit_logger(&config.project_root, &session.id, &settings.model.model);

        let initial_permission_mode = permission_mode_from_str(&settings.permissions.mode);

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

    /// Build system prompt from fresh dynamic context (Progress, Memory, INDEX, Skills).
    /// `permission_mode`: settings `mode` string — "unattended" enables autonomous writing prompt.
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

    /// Snapshot for Tauri / frontend status bar.
    pub fn status_snapshot(&self) -> EngineStatus {
        let mode = self.tool_context().effective_permission_mode();
        EngineStatus {
            session_id: self.shared.session.id.clone(),
            permission_mode: permission_mode_label(&mode).to_string(),
            hook_running: self.shared.drain_in_progress.load(Ordering::SeqCst),
            pending_user_question: self.pending_user_question.is_some(),
            turn_number: self.turn_number,
            project_initialized: self.shared.session.project_root.join("AGENTS.md").is_file(),
            has_interruptible_tool_in_progress: self.has_interruptible_tool_in_progress,
        }
    }

    pub fn set_permission_mode_override(&self, mode: novel_tools::PermissionMode) {
        if let Ok(mut g) = self.shared.permission_mode_override.lock() {
            *g = mode;
        }
    }

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
            permission_mode: permission_mode_from_str(&self.shared.settings.permissions.mode),
            deny_rules: self.shared.settings.permissions.deny_rules.clone(),
            always_allow: crate::agent::merge_tool_always_allow(
                &self.shared.settings.permissions.always_allow,
            ),
            project_root: self.shared.session.project_root.clone(),
            session_id: self.shared.session.id.clone(),
            db: Some(Arc::new(self.shared.session.db.clone())),
            permission_mode_override: Some(Arc::clone(&self.shared.permission_mode_override)),
            read_file_cache: Some(Arc::clone(&self.shared.read_file_cache)),
            allow_fork: self.shared.sub_agent_count.load(Ordering::SeqCst) == 0,
            subagent_queue: Some(Arc::clone(&self.shared.subagent_queue)),
            current_tool_call_id: None,
            skills_dir: Some(self.shared.agent_skills_dir.clone()),
            global_api_config_path: Some(self.shared.global_config_path.clone()),
        }
    }

    pub fn tool_context_dont_ask(&self) -> ToolContext {
        let ctx = self.tool_context();
        if let Some(lock) = &ctx.permission_mode_override {
            if let Ok(mut guard) = lock.lock() {
                *guard = PermissionMode::Auto;
            }
        }
        ctx
    }

    // ── Sub-agent management (used by turn_loop.rs) ───────────

    /// Increment the running sub-agent count (called before spawn).
    pub fn sub_agent_inc(&self) {
        self.shared.sub_agent_count.fetch_add(1, Ordering::SeqCst);
    }

    /// Decrement the running sub-agent count (called when a spawned task completes).
    pub fn sub_agent_dec(&self) {
        self.shared.sub_agent_count.fetch_sub(1, Ordering::SeqCst);
    }

    // ── Run loop (CLI / non-Tauri) ────────────────────────────

    pub async fn run(
        mut self,
        mut op_rx: mpsc::UnboundedReceiver<Op>,
        event_tx: mpsc::UnboundedSender<Event>,
    ) -> Result<TerminalReason, AgentError> {
        while let Some(op) = op_rx.recv().await {
            match op {
                Op::SendMessage { content, model } => {
                    let reason = self
                        .handle_message_with_events(&content, model.as_deref(), Some(&event_tx))
                        .await?;
                    if !matches!(reason, TerminalReason::Completed) {
                        return Ok(reason);
                    }
                }
                Op::Interrupt => {
                    self.shared
                        .abort_controller
                        .request(crate::InterruptReason::UserCancel);
                    return Ok(TerminalReason::AbortedStreaming);
                }
                Op::ApproveTool { tool_call_id } => {
                    self.approve_tool(&tool_call_id, Some(&event_tx)).await?;
                }
                Op::DenyTool {
                    tool_call_id,
                    reason,
                } => {
                    self.deny_tool(&tool_call_id, reason, Some(&event_tx))
                        .await?;
                }
                Op::ResumeSession { session_id } => {
                    if session_id != self.shared.session.id {
                        return Err(AgentError::Validation(
                            "resume session id mismatch in run loop".into(),
                        ));
                    }
                }
            }
        }
        Ok(TerminalReason::Completed)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ForkError;
    use rstest::rstest;
    use tempfile::TempDir;

    fn test_config(tmp: &TempDir) -> EngineConfig {
        EngineConfig {
            project_root: tmp.path().to_path_buf(),
            settings_path: tmp.path().join("settings.json"),
            db_path: tmp.path().join("state.db"),
            skills_dir: tmp.path().join("skills"),
            global_config_path: tmp.path().join(".novel-agent/api_config.json"),
        }
    }

    #[rstest]
    #[tokio::test]
    async fn empty_message_returns_validation_error() {
        let tmp = TempDir::new().unwrap();
        std::fs::create_dir_all(tmp.path().join("skills")).unwrap();
        let mut engine = AgentEngine::new(test_config(&tmp)).unwrap();
        let err = engine.handle_message("").await.unwrap_err();
        assert!(matches!(err, AgentError::Validation(_)));
    }

    #[rstest]
    #[tokio::test]
    async fn nested_fork_prohibited_when_sub_agent_running() {
        let tmp = TempDir::new().unwrap();
        std::fs::create_dir_all(tmp.path().join("skills")).unwrap();
        let engine = AgentEngine::new(test_config(&tmp)).unwrap();
        engine.shared.sub_agent_count.store(1, Ordering::SeqCst);
        let err = engine
            .fork(AgentType::ChapterCraftAnalyzer, "分析第31章".into())
            .await
            .unwrap_err();
        assert!(matches!(err, AgentError::NestedForkProhibited));
    }

    #[rstest]
    #[tokio::test]
    async fn nested_fork_prohibited() {
        let tmp = TempDir::new().unwrap();
        std::fs::create_dir_all(tmp.path().join("skills")).unwrap();
        let engine = AgentEngine::new(test_config(&tmp)).unwrap();
        let child = engine
            .fork(AgentType::KnowledgeAuditor, "审计第31章".into())
            .await
            .unwrap();
        assert!(child.is_child);
    }

    #[rstest]
    #[tokio::test]
    async fn fork_empty_task_errors() {
        let tmp = TempDir::new().unwrap();
        std::fs::create_dir_all(tmp.path().join("skills")).unwrap();
        let engine = AgentEngine::new(test_config(&tmp)).unwrap();
        let err = engine
            .fork(AgentType::KnowledgeAuditor, "  ".into())
            .await
            .unwrap_err();
        assert!(matches!(err, AgentError::Fork(ForkError::EmptyTask)));
    }

    #[rstest]
    #[tokio::test]
    async fn engine_run_handles_message() {
        let _offline = crate::test_env::StripDeepseekApiKey::new();
        let tmp = TempDir::new().unwrap();
        std::fs::create_dir_all(tmp.path().join("skills")).unwrap();
        let engine = AgentEngine::new(test_config(&tmp)).unwrap();
        let (op_tx, op_rx) = mpsc::unbounded_channel();
        let (event_tx, mut event_rx) = mpsc::unbounded_channel();
        op_tx
            .send(Op::SendMessage {
                content: "你好".into(),
                model: None,
            })
            .unwrap();
        drop(op_tx);
        let handle = tokio::spawn(async move { engine.run(op_rx, event_tx).await });
        let mut saw_turn = false;
        while let Some(ev) = event_rx.recv().await {
            if matches!(ev, Event::TurnComplete { .. }) {
                saw_turn = true;
            }
        }
        assert!(saw_turn);
        assert!(handle.await.unwrap().is_ok());
    }

    #[rstest]
    #[tokio::test]
    async fn engine_run_approve_tool_op() {
        let _offline = crate::test_env::StripDeepseekApiKey::new();
        let tmp = TempDir::new().unwrap();
        std::fs::create_dir_all(tmp.path().join("skills")).unwrap();
        let mut engine = AgentEngine::new(test_config(&tmp)).unwrap();
        engine.pending_tools.insert(
            "p1".into(),
            novel_tools::ToolCallSpec {
                id: "p1".into(),
                name: "Read".into(),
                input: serde_json::json!({"file_path": "settings.json"}),
            },
        );
        let (op_tx, op_rx) = mpsc::unbounded_channel();
        let (event_tx, _event_rx) = mpsc::unbounded_channel();
        op_tx
            .send(Op::ApproveTool {
                tool_call_id: "p1".into(),
            })
            .unwrap();
        drop(op_tx);
        let reason = engine.run(op_rx, event_tx).await.unwrap();
        assert!(matches!(reason, TerminalReason::Completed));
    }

    #[rstest]
    #[tokio::test]
    async fn resume_session_loads_messages() {
        let _offline = crate::test_env::StripDeepseekApiKey::new();
        let tmp = TempDir::new().unwrap();
        std::fs::create_dir_all(tmp.path().join("skills")).unwrap();
        let cfg = test_config(&tmp);
        let mut engine = AgentEngine::new(cfg.clone()).unwrap();
        engine.handle_message("第一条").await.unwrap();
        let sid = engine.shared.session.id.clone();
        let resumed = AgentEngine::resume(cfg, &sid).unwrap();
        assert!(resumed.messages.len() >= 2);
    }

    #[test]
    fn permission_mode_label_maps_all_modes() {
        use novel_tools::PermissionMode;
        assert_eq!(permission_mode_label(&PermissionMode::Normal), "normal");
        assert_eq!(permission_mode_label(&PermissionMode::Auto), "auto");
        assert_eq!(permission_mode_label(&PermissionMode::Plan), "plan");
        assert_eq!(
            permission_mode_label(&PermissionMode::Unattended),
            "unattended"
        );
    }

    #[test]
    fn clear_read_file_cache_removes_all_entries() {
        use novel_tools::{ReadCacheEntry, ReadCacheSource};
        use std::path::PathBuf;

        let tmp = TempDir::new().unwrap();
        std::fs::create_dir_all(tmp.path().join("skills")).unwrap();
        let engine = AgentEngine::new(test_config(&tmp)).unwrap();
        engine.shared.read_file_cache.insert(
            PathBuf::from("a.md"),
            ReadCacheEntry {
                mtime_secs: 1,
                raw_content: "x".into(),
                offset: None,
                limit: None,
                total_lines: 1,
                source: ReadCacheSource::WriteRefresh,
            },
        );
        assert_eq!(engine.shared.read_file_cache.len(), 1);
        engine.shared.clear_read_file_cache();
        assert!(engine.shared.read_file_cache.is_empty());
    }
}
