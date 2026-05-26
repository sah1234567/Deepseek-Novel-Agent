use crate::{
    AgentError, AgentType, ChatMessage, ContextManager, DynamicContext, Event, ForkError,
    ForkedAgentContext, Op, SessionHandle, SystemPromptBuilder, TerminalReason,
};

use crate::dynamic_context::build_dynamic_context;
use crate::hooks::default_hook_config;
use crate::interrupt::AbortController;
use crate::message_bridge::{repair_tool_use_chain, stored_to_chat};

use novel_config::{load_project_settings, ProjectSettings};

use novel_knowledge::KnowledgeStore;

use novel_deepseek::ChatClient;
use novel_logging::{AuditLogger, LogEvent};

use novel_tools::{
    default_registry, ForkQueue, PermissionMode, ReadCacheEntry, ToolCallSpec, ToolContext,
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
    pub hook_running: bool,
    pub pending_user_question: bool,
    pub turn_number: u32,
    pub project_initialized: bool,
    pub has_interruptible_tool_in_progress: bool,
}

fn permission_mode_label(mode: &PermissionMode) -> &'static str {
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
    pub fork_queue: ForkQueue,
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

    // Accessors for backward compatibility
    pub messages: Vec<ChatMessage>,
    pub is_forked_child: bool,
    pub is_streaming: bool,
    pub turn_number: u32,
    pub(crate) pending_tools: HashMap<String, ToolCallSpec>,
    pub(crate) hook_running: bool,
    pub(crate) pending_user_question: Option<String>,
    pub(crate) last_turn_usage: Option<novel_deepseek::TokenUsage>,
    pub(crate) last_context_tokens: usize,
    pub(crate) has_interruptible_tool_in_progress: bool,
    pub(crate) active_sub_agent: Option<AgentType>,
    pub(crate) llm: Option<ChatClient>,
    pub(crate) invoked_skill_ids: Vec<String>,
    pub(crate) read_skill_reference_paths: Vec<String>,
    /// PostToolUse auto-trigger: KnowledgeAuditor subagent task prompts (drain via `drain_pending_hooks`).
    pub(crate) pending_hook_tasks: Vec<String>,
    pub(crate) last_chapter_written: Option<String>,
    pub(crate) compaction_fail_count: u32,
    /// Monotonic `(turn_number, sequence)` counter for the active user turn.
    /// User message is `0`; assistant/tool messages use `1, 2, 3…` in chat order.
    pub(crate) turn_message_seq: i32,

    /// Legacy channel for debug IPC `ForkSubAgent`; no consumer injects into parent session.
    pub subagent_result_rx: mpsc::UnboundedReceiver<(AgentType, String)>,
    pub subagent_result_tx: mpsc::UnboundedSender<(AgentType, String)>,
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

        let registry = Arc::new(default_registry(config.project_root.clone()));
        let context_manager = ContextManager::new(&settings.model);
        let (system_prompt, agents_md) =
            Self::build_initial_prompt(&config, &settings, &session)?;

        let messages = vec![ChatMessage {
            role: "system".into(),
            content: system_prompt.clone(),
            tool_call_id: None,
            tool_calls: None,
            reasoning_content: None,
        }];

        let (tx, rx) = mpsc::unbounded_channel();

        let audit = open_audit_logger(
            &config.project_root,
            &session.id,
            &settings.model.model,
        );

        let initial_permission_mode =
            permission_mode_from_str(&settings.permissions.mode);

        let shared = EngineShared {
            session,
            settings: Arc::new(settings),
            registry,
            context_manager,
            abort_controller,
            permission_mode_override: Arc::new(Mutex::new(initial_permission_mode)),
            read_file_cache: Arc::new(DashMap::new()),
            fork_queue: Arc::new(Mutex::new(Vec::new())),
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
            is_forked_child: false,
            is_streaming: false,
            turn_number: 0,
            pending_tools: HashMap::new(),
            hook_running: false,
            pending_user_question: None,
            last_turn_usage: None,
            last_context_tokens: 0,
            has_interruptible_tool_in_progress: false,
            active_sub_agent: None,
            llm: None,
            invoked_skill_ids: Vec::new(),
            read_skill_reference_paths: Vec::new(),
            pending_hook_tasks: Vec::new(),
            last_chapter_written: None,
            compaction_fail_count: 0,
            turn_message_seq: 0,
            subagent_result_rx: rx,
            subagent_result_tx: tx,
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

        let registry = Arc::new(default_registry(config.project_root.clone()));
        let context_manager = ContextManager::new(&settings.model);
        let stored = session.db.get_session_messages(session_id, None)?;

        let messages = if stored.is_empty() {
            let (system_prompt, agents_md) =
                Self::build_initial_prompt(&config, &settings, &session)?;
            (
                vec![ChatMessage {
                    role: "system".into(),
                    content: system_prompt.clone(),
                    tool_call_id: None,
                    tool_calls: None,
                    reasoning_content: None,
                }],
                system_prompt,
                agents_md,
            )
        } else {
            let mut sp = stored_to_chat(&stored);
            repair_tool_use_chain(&mut sp);
            let sys = sp
                .first()
                .filter(|m| m.role == "system")
                .map(|m| m.content.clone())
                .unwrap_or_default();
            (sp, sys.clone(), String::new())
        };

        let (messages, system_prompt, agents_md) = messages;

        let system_prompt = if system_prompt.is_empty() {
            Self::build_initial_prompt(&config, &settings, &session)
                .map(|(p, _)| p)
                .unwrap_or_default()
        } else {
            system_prompt
        };

        let turn_number = stored.iter().map(|m| m.turn_number).max().unwrap_or(0) as u32;

        let invoked_skill_ids = session
            .db
            .get_invoked_skill_ids(session_id)
            .unwrap_or_default();

        let read_skill_reference_paths = session
            .db
            .get_read_skill_reference_paths(session_id)
            .unwrap_or_default();

        let (tx, rx) = mpsc::unbounded_channel();

        let audit = open_audit_logger(
            &config.project_root,
            &session.id,
            &settings.model.model,
        );

        let initial_permission_mode =
            permission_mode_from_str(&settings.permissions.mode);

        let shared = EngineShared {
            session,
            settings: Arc::new(settings),
            registry,
            context_manager,
            abort_controller,
            permission_mode_override: Arc::new(Mutex::new(initial_permission_mode)),
            read_file_cache: Arc::new(DashMap::new()),
            fork_queue: Arc::new(Mutex::new(Vec::new())),
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
            is_forked_child: false,
            is_streaming: false,
            turn_number,
            pending_tools: HashMap::new(),
            hook_running: false,
            pending_user_question: None,
            last_turn_usage: None,
            last_context_tokens: 0,
            has_interruptible_tool_in_progress: false,
            active_sub_agent: None,
            llm: None,
            invoked_skill_ids,
            read_skill_reference_paths,
            pending_hook_tasks: Vec::new(),
            last_chapter_written: None,
            compaction_fail_count: 0,
            turn_message_seq: 0,
            subagent_result_rx: rx,
            subagent_result_tx: tx,
        })
    }

    fn build_initial_prompt(
        config: &EngineConfig,
        _settings: &ProjectSettings,
        session: &SessionHandle,
    ) -> Result<(String, String), AgentError> {
        let store = KnowledgeStore::new(&config.project_root);
        let agents = store
            .read_file("AGENTS.md")
            .unwrap_or_else(|_| "默认：第三人称限知，2000-3000字/章".into());
        let (prompt, _) = Self::assemble_system_prompt(config, session, &agents)?;
        Ok((prompt, agents))
    }

    /// Build system prompt from fresh dynamic context (Progress, Memory, INDEX, Skills).
    pub fn assemble_system_prompt(
        config: &EngineConfig,
        session: &SessionHandle,
        agents_md: &str,
    ) -> Result<(String, DynamicContext), AgentError> {
        let dynamic = build_dynamic_context(
            &config.project_root,
            &session.id,
            &session.db,
            agents_md,
            &config.skills_dir,
        );
        let prompt = SystemPromptBuilder::new().build(&dynamic);
        Ok((prompt, dynamic))
    }

    /// Refresh `messages[0]` and shared system prompt after compaction.
    pub fn rebuild_system_message(&mut self) -> Result<(), AgentError> {
        let dynamic = build_dynamic_context(
            &self.shared.session.project_root,
            &self.shared.session.id,
            &self.shared.session.db,
            &self.shared.agents_md,
            &self.shared.agent_skills_dir,
        );
        let prompt = SystemPromptBuilder::new().build(&dynamic);
        self.shared.system_prompt = prompt.clone();
        if let Some(m0) = self.messages.first_mut() {
            if m0.role == "system" {
                m0.content = prompt;
            }
        }
        Ok(())
    }

    pub fn is_forked_child(&self) -> bool {
        self.is_forked_child
    }

    pub fn system_prompt(&self) -> &str {
        &self.shared.system_prompt
    }

    /// Snapshot for Tauri / frontend status bar.
    pub fn status_snapshot(&self) -> EngineStatus {
        let mode = self.tool_context().effective_permission_mode();
        EngineStatus {
            session_id: self.shared.session.id.clone(),
            permission_mode: permission_mode_label(&mode).to_string(),
            hook_running: self.hook_running,
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
        self.handle_message_with_events(content, None).await
    }

    // ── Fork ──────────────────────────────────────────────────

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
        if self.is_forked_child {
            tracing::warn!("fork rejected: nested fork prohibited");
            return Err(AgentError::NestedForkProhibited);
        }
        if self.shared.sub_agent_count.load(Ordering::SeqCst) > 0 {
            // Still block nested forks from the main agent context
            if self.active_sub_agent.is_some() {
                return Err(AgentError::NestedForkProhibited);
            }
        }

        let store = KnowledgeStore::new(&self.shared.session.project_root);
        let mut snapshots = HashMap::new();
        if let Ok(index) = store.read_file("knowledge/INDEX.md") {
            snapshots.insert(PathBuf::from("knowledge/INDEX.md"), index);
        }

        ForkedAgentContext::fork(
            self.messages
                .first()
                .ok_or(AgentError::Validation("no system message".into()))?,
            self.shared.session.id.clone(),
            agent_type,
            task_prompt,
            agent_type.max_react_loops_for(&self.shared.settings.agent),
            snapshots,
            self.is_forked_child,
        )
        .map_err(|e| {
            if e == ForkError::InvalidMaxReactLoops(0) {
                AgentError::NestedForkProhibited
            } else {
                AgentError::Fork(e)
            }
        })
    }

    // ── Tool context ──────────────────────────────────────────

    pub fn tool_context(&self) -> ToolContext {
        ToolContext {
            permission_mode: permission_mode_from_str(&self.shared.settings.permissions.mode),
            deny_rules: self.shared.settings.permissions.deny_rules.clone(),
            always_allow: self.shared.settings.permissions.always_allow.clone(),
            project_root: self.shared.session.project_root.clone(),
            session_id: self.shared.session.id.clone(),
            db: Some(Arc::new(self.shared.session.db.clone())),
            permission_mode_override: Some(Arc::clone(&self.shared.permission_mode_override)),
            read_file_cache: Some(Arc::clone(&self.shared.read_file_cache)),
            allow_fork: self.shared.sub_agent_count.load(Ordering::SeqCst) == 0,
            fork_queue: Some(Arc::clone(&self.shared.fork_queue)),
            skills_dir: Some(self.shared.agent_skills_dir.clone()),
        }
    }

    /// PostToolUse auto-trigger: runs KnowledgeAuditor subagent synchronously.
    /// Does not inject report into parent `messages` (avoids polluting main LLM context).
    pub async fn run_knowledge_auditor_hook(
        &mut self,
        task: String,
        event_tx: Option<&mpsc::UnboundedSender<Event>>,
    ) -> Result<(), AgentError> {
        if self.hook_running {
            tracing::debug!("knowledge_auditor_hook skipped: hook already running");
            return Ok(());
        }
        let trigger_preview: String = task.chars().take(120).collect();
        tracing::debug!(
            session_id = %self.shared.session.id,
            trigger_preview = %trigger_preview,
            "knowledge_auditor_hook_start"
        );
        self.audit_log(LogEvent::KnowledgeAuditorHookForked {
            session_id: self.shared.session.id.clone(),
            trigger_tool: trigger_preview,
        });
        self.hook_running = true;
        let fork_run_id = crate::fork_transcript::create_fork_run(
            &self.shared.session.db,
            &self.shared.session.id,
            self.turn_number as i32,
            "KnowledgeAuditor",
            &task,
            "hook",
        )?;
        let result = async {
            let mut child = self
                .fork(AgentType::KnowledgeAuditor, task)
                .await?;
            let saved = self.shared.permission_mode_override.clone();
            if let Ok(mut g) = self.shared.permission_mode_override.lock() {
                *g = PermissionMode::Auto;
            }
            let out = self
                .run_forked_agent(&mut child, &fork_run_id, event_tx)
                .await;
            if let Ok(mut g) = saved.lock() {
                *g = PermissionMode::Normal;
            }
            out.map(|_| ())
        }
        .await;
        self.hook_running = false;
        let status = if result.is_ok() { "complete" } else { "failed" };
        let _ = crate::fork_transcript::finish_fork_run(
            &self.shared.session.db,
            &fork_run_id,
            status,
            None,
        );
        match &result {
            Ok(()) => tracing::debug!(
                session_id = %self.shared.session.id,
                "knowledge_auditor_hook_complete"
            ),
            Err(e) => {
                tracing::warn!(error = %e, "knowledge_auditor_hook_failed");
                self.audit_error(e.to_string(), true);
            }
        }
        result
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
                Op::SendMessage { content } => {
                    let reason = self
                        .handle_message_with_events(&content, Some(&event_tx))
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
                Op::DenyTool { tool_call_id, reason } => {
                    self.deny_tool(&tool_call_id, reason, Some(&event_tx)).await?;
                }
                Op::ResumeSession { session_id } => {
                    if session_id != self.shared.session.id {
                        return Err(AgentError::Validation(
                            "resume session id mismatch in run loop".into(),
                        ));
                    }
                }
                Op::ForkSubAgent { agent_type, task_prompt } => {
                    let mut child = self.fork(agent_type, task_prompt).await?;
                    let fork_run_id = crate::fork_transcript::create_fork_run(
                        &self.shared.session.db,
                        &self.shared.session.id,
                        self.turn_number as i32,
                        &agent_type.to_string(),
                        &child.fork.task_message.content,
                        "tool",
                    )?;
                    let output = self
                        .run_forked_agent(&mut child, &fork_run_id, Some(&event_tx))
                        .await?;
                    self.run_post_fork_pipeline(agent_type, &output, Some(&fork_run_id))
                        .await?;
                }
            }
        }
        Ok(TerminalReason::Completed)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
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
        let mut engine = AgentEngine::new(test_config(&tmp)).unwrap();
        engine.active_sub_agent = Some(AgentType::KnowledgeAuditor);
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
            .send(Op::SendMessage { content: "你好".into() })
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
