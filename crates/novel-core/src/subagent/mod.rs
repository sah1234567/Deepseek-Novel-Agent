//! Unified async subagent queue, drain, and per-job ReAct runner.

mod fork_setup;
pub(crate) mod fork_transcript;
mod helpers;
mod llm_turn;
mod memory_extractor;
mod overflow;
mod params;
mod react;
mod runner;

pub use fork_setup::{build_fork_child, ForkError, ForkedAgentContext};
pub use memory_extractor::spawn_memory_extraction;
pub use runner::run_subagent_job;

use crate::engine::session_llm::read_session_llm;
use crate::{AgentEngine, AgentError, AgentType, Event};
use novel_knowledge::{mark_audited, parse_chapter_numbers, AuditKind, KnowledgeStore};
use novel_tools::{PendingSubagentWork, PermissionMode};
use std::sync::atomic::Ordering;
use std::sync::Arc;
use tokio::sync::mpsc;

#[derive(Debug, Clone)]
pub enum SubagentJobKind {
    ToolFork {
        parent_tool_call_id: String,
    },
    HookAuditor,
    /// Background extractMemories (no UI overlay, no parent report injection).
    MemoryExtraction,
}

#[derive(Debug, Clone)]
pub struct SubagentJob {
    pub agent_type: AgentType,
    pub task: String,
    pub kind: SubagentJobKind,
}

impl SubagentJob {
    pub fn from_pending(work: PendingSubagentWork) -> Result<Self, AgentError> {
        let agent_type = AgentType::parse(&work.agent_type).ok_or_else(|| {
            AgentError::Validation(format!("unknown fork agentType: {}", work.agent_type))
        })?;
        let kind = match work.parent_tool_call_id {
            Some(parent_tool_call_id) => SubagentJobKind::ToolFork {
                parent_tool_call_id,
            },
            None => SubagentJobKind::HookAuditor,
        };
        Ok(Self {
            agent_type,
            task: work.task,
            kind,
        })
    }

    pub fn inject_report(&self) -> bool {
        matches!(self.kind, SubagentJobKind::ToolFork { .. })
    }
}

pub fn clear_subagent_queue(shared: &crate::EngineShared) {
    if let Ok(mut guard) = shared.subagent_queue.lock() {
        guard.clear();
    }
}

fn take_queued_jobs(engine: &AgentEngine) -> Result<Vec<SubagentJob>, AgentError> {
    let mut guard = engine
        .shared
        .subagent_queue
        .lock()
        .map_err(|_| AgentError::Validation("subagent queue lock poisoned".into()))?;
    std::mem::take(&mut *guard)
        .into_iter()
        .map(SubagentJob::from_pending)
        .collect()
}

fn log_hook_batch_fork(engine: &AgentEngine, jobs: &[SubagentJob]) {
    let hook_count = jobs
        .iter()
        .filter(|j| matches!(j.kind, SubagentJobKind::HookAuditor))
        .count();
    engine
        .shared
        .audit_log(&novel_logging::LogEvent::KnowledgeAuditorHookForked {
            session_id: engine.shared.session.id.clone(),
            trigger_tool: format!("batch: {hook_count} hook job(s)"),
        });
}

fn set_permission_override(shared: &crate::EngineShared, mode: PermissionMode) {
    if let Ok(mut g) = shared.permission_mode_override.lock() {
        *g = mode;
    }
}

/// Restores hook-batch permission override on drop (including error paths).
struct PermissionOverrideGuard {
    shared: crate::EngineShared,
    prior: PermissionMode,
    active: bool,
}

impl PermissionOverrideGuard {
    fn new(shared: &crate::EngineShared, hook_batch: bool) -> Self {
        let prior = shared
            .permission_mode_override
            .lock()
            .map(|g| g.clone())
            .unwrap_or(PermissionMode::Normal);
        if hook_batch {
            set_permission_override(shared, PermissionMode::Auto);
        }
        Self {
            shared: shared.clone(),
            prior,
            active: hook_batch,
        }
    }
}

impl Drop for PermissionOverrideGuard {
    fn drop(&mut self) {
        if self.active {
            set_permission_override(&self.shared, self.prior.clone());
        }
    }
}

struct DrainInProgressGuard(Arc<std::sync::atomic::AtomicBool>);

impl Drop for DrainInProgressGuard {
    fn drop(&mut self) {
        self.0.store(false, Ordering::SeqCst);
    }
}

fn record_subagent_audit(engine: &AgentEngine, agent_type: AgentType, task: &str) {
    if matches!(agent_type, AgentType::GeneralPurpose) {
        return;
    }
    let Some(kind) = AuditKind::from_agent_name(&agent_type.to_string()) else {
        return;
    };
    let chapters = parse_chapter_numbers(task);
    if chapters.is_empty() {
        tracing::debug!(
            agent = %agent_type,
            "audit_status_skip_no_chapter_in_task"
        );
        return;
    }
    let store = KnowledgeStore::new(&engine.shared.session.project_root);
    match mark_audited(&store, kind, &chapters, task) {
        Ok(()) => tracing::debug!(
            agent = %agent_type,
            chapters = ?chapters,
            "audit_status_marked_audited"
        ),
        Err(e) => tracing::warn!(
            agent = %agent_type,
            error = %e,
            "audit_status_mark_failed"
        ),
    }
}

async fn apply_subagent_success(
    engine: &mut AgentEngine,
    agent_type: AgentType,
    fork_run_id: &str,
    inject: bool,
    task: &str,
    output: &str,
    event_tx: Option<&mpsc::UnboundedSender<Event>>,
) -> Result<(), AgentError> {
    if inject {
        record_subagent_audit(engine, agent_type, task);
        engine.inject_sub_agent_report(agent_type, output, Some(fork_run_id))?;
        if engine.compaction_needed() {
            engine.compact_with_events(event_tx).await;
        }
    } else if let Err(e) =
        fork_transcript::finish_fork_run(&engine.shared.session.db, fork_run_id, "complete", None)
    {
        tracing::warn!(
            fork_run_id = %fork_run_id,
            error = %e,
            "finish_fork_run failed for hook subagent"
        );
    }
    Ok(())
}

async fn join_subagent_handles(
    engine: &mut AgentEngine,
    handles: Vec<tokio::task::JoinHandle<(AgentType, Result<String, AgentError>)>>,
    meta: Vec<(AgentType, String, bool, String)>,
    event_tx: Option<&mpsc::UnboundedSender<Event>>,
) -> Result<(), AgentError> {
    for (i, handle) in handles.into_iter().enumerate() {
        let (_agent_type, fork_run_id, inject, task) = meta.get(i).cloned().unwrap_or((
            AgentType::KnowledgeAuditor,
            String::new(),
            false,
            String::new(),
        ));
        match handle.await {
            Ok((at, Ok(output))) => {
                apply_subagent_success(engine, at, &fork_run_id, inject, &task, &output, event_tx)
                    .await?;
            }
            Ok((_, Err(e))) => {
                tracing::error!(error = %e, "subagent_job_failed");
            }
            Err(e) => {
                return Err(AgentError::Validation(format!(
                    "subagent task join failed: {e}"
                )));
            }
        }
    }
    Ok(())
}

pub async fn drain_subagent_jobs(
    engine: &mut AgentEngine,
    event_tx: Option<&mpsc::UnboundedSender<Event>>,
) -> Result<(), AgentError> {
    let jobs = take_queued_jobs(engine)?;
    if jobs.is_empty() {
        return Ok(());
    }

    let llm_snap = read_session_llm(&engine.shared);
    let hook_batch = jobs
        .iter()
        .any(|j| matches!(j.kind, SubagentJobKind::HookAuditor));
    if hook_batch {
        log_hook_batch_fork(engine, &jobs);
    }

    let _perm_guard = PermissionOverrideGuard::new(&engine.shared, hook_batch);

    engine
        .shared
        .drain_in_progress
        .store(true, Ordering::SeqCst);
    let _guard = DrainInProgressGuard(Arc::clone(&engine.shared.drain_in_progress));

    let mut handles = Vec::with_capacity(jobs.len());
    let mut meta: Vec<(AgentType, String, bool, String)> = Vec::with_capacity(jobs.len());
    for job in jobs {
        let agent_type = job.agent_type;
        let task = job.task.clone();
        let inject = job.inject_report();
        let fork_run_id = fork_transcript::create_fork_run(
            &engine.shared.session.db,
            &engine.shared.session.id,
            engine.turn_number as i32,
            &agent_type.to_string(),
            &job.task,
            if inject { "tool" } else { "hook" },
        )?;
        let shared = engine.shared.clone();
        let snap = llm_snap.clone();
        let job_for_spawn = job.clone();
        engine.sub_agent_inc();
        let event_tx_clone = event_tx.cloned();
        meta.push((agent_type, fork_run_id.clone(), inject, task));
        handles.push(tokio::spawn(async move {
            let result =
                run_subagent_job(shared, job_for_spawn, fork_run_id, snap, event_tx_clone).await;
            (agent_type, result)
        }));
    }

    join_subagent_handles(engine, handles, meta, event_tx).await?;
    Ok(())
}

#[cfg(test)]
mod permission_guard_tests {
    use super::*;
    use crate::EngineConfig;
    use tempfile::TempDir;

    #[test]
    fn permission_override_guard_restores_on_drop() {
        let tmp = TempDir::new().unwrap();
        std::fs::create_dir_all(tmp.path().join("skills")).unwrap();
        let engine = crate::AgentEngine::new(EngineConfig {
            project_root: tmp.path().to_path_buf(),
            settings_path: tmp.path().join("settings.json"),
            db_path: tmp.path().join("state.db"),
            skills_dir: tmp.path().join("skills"),
            global_config_path: tmp.path().join(".novel-agent/api_config.json"),
        })
        .unwrap();
        set_permission_override(&engine.shared, PermissionMode::Plan);
        {
            let _guard = PermissionOverrideGuard::new(&engine.shared, true);
            assert_eq!(
                engine
                    .shared
                    .permission_mode_override
                    .lock()
                    .map(|g| g.clone())
                    .unwrap(),
                PermissionMode::Auto
            );
        }
        assert_eq!(
            engine
                .shared
                .permission_mode_override
                .lock()
                .map(|g| g.clone())
                .unwrap(),
            PermissionMode::Plan
        );
    }
}
