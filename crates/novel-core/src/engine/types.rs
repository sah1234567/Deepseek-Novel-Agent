use crate::engine::session_llm::{write_session_llm, SessionLlm, SessionLlmSnapshot};
use crate::fork_stream_subs::ForkStreamSubscriptions;
use crate::interrupt::AbortController;
use crate::{ChatMessage, ContextManager, SessionHandle};

use novel_config::ProjectSettings;
use novel_deepseek::ChatClient;
use novel_logging::{AuditLogger, LogEvent};
use novel_tools::{PermissionMode, ReadCacheEntry, SubagentWorkQueue, ToolCallSpec, ToolRegistry};

use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::sync::atomic::AtomicU32;
use std::sync::{Arc, Mutex};

use dashmap::DashMap;
use serde::Serialize;

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct EngineStatus {
    pub session_id: String,
    pub permission_mode: String,
    /// True while `drain_subagent_jobs` is active (PostToolUse hook batch and/or tool forks).
    pub hook_running: bool,
    pub pending_user_question: bool,
    /// True while a turn is paused (pending question/approval) or a turn command is running.
    pub turn_in_progress: bool,
    pub turn_number: u32,
    pub project_initialized: bool,
    pub has_interruptible_tool_in_progress: bool,
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
    /// Paths touched since last SQLite flush (partial UPSERT batch).
    pub read_cache_dirty_paths: Arc<Mutex<HashSet<PathBuf>>>,
    pub file_op_locks: Arc<DashMap<PathBuf, Arc<tokio::sync::Mutex<()>>>>,
    pub subagent_queue: SubagentWorkQueue,
    pub session_llm: SessionLlm,
    pub drain_in_progress: Arc<std::sync::atomic::AtomicBool>,
    pub agents_md: String,
    pub agent_skills_dir: PathBuf,
    pub global_config_path: PathBuf,
    pub system_prompt: String,
    pub sub_agent_count: Arc<AtomicU32>,
    /// UI overlay subscriptions; gates fork stream/tool IPC at source.
    pub fork_stream_subs: ForkStreamSubscriptions,

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

    /// Clear in-memory read cache and persisted `session_read_cache` rows (tests / explicit reset).
    pub fn clear_read_file_cache(&self) {
        if let Err(e) = crate::read_cache::sync::clear_read_file_cache_persisted(self) {
            tracing::warn!(error = %e, "clear_read_file_cache failed");
        }
    }

    /// Increment the running sub-agent count (called before spawn).
    pub fn sub_agent_inc(&self) {
        self.sub_agent_count
            .fetch_add(1, std::sync::atomic::Ordering::SeqCst);
    }

    /// Decrement the running sub-agent count (called when a spawned task completes).
    pub fn sub_agent_dec(&self) {
        self.sub_agent_count
            .fetch_sub(1, std::sync::atomic::Ordering::SeqCst);
    }
}

pub(crate) fn open_audit_logger(
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
    /// Consecutive identical tool failure signature within the active turn.
    pub(crate) consecutive_tool_failure_key: Option<String>,
    pub(crate) consecutive_tool_failure_count: u32,
    /// Monotonic `(turn_number, sequence)` counter for the active user turn.
    /// User message is `0`; assistant/tool messages use `1, 2, 3…` in chat order.
    pub(crate) turn_message_seq: i32,
    /// Enter/exit Unattended copy merged into the **next** user message (not a separate row).
    pub(crate) pending_permission_user_prefix: Option<String>,
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

    pub fn session_id(&self) -> &str {
        &self.shared.session.id
    }

    pub fn list_session_todos(&self) -> Vec<novel_state::SessionTodo> {
        self.shared
            .session
            .db
            .list_session_todos(self.session_id())
            .unwrap_or_default()
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

    /// Re-attach the shared subscription set after engine replace (resume / new session).
    pub fn attach_fork_stream_subs(&mut self, subs: ForkStreamSubscriptions) {
        self.shared.fork_stream_subs = subs;
    }
}
