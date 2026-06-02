//! Unified async subagent queue, drain, and per-job ReAct runner.

use crate::fork_transcript;
use crate::session_llm::read_session_llm;
use crate::{
    AgentEngine, AgentError, AgentType, ChatMessage, Event, ForkError, ForkedAgentContext,
};
use novel_tools::{PendingSubagentWork, PermissionMode};
use std::sync::atomic::Ordering;
use std::sync::Arc;
use tokio::sync::mpsc;

pub use crate::subagent_runner::run_subagent_job;

#[derive(Debug, Clone)]
pub enum SubagentJobKind {
    ToolFork { parent_tool_call_id: String },
    HookAuditor,
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

pub fn build_fork_child(
    shared: &crate::EngineShared,
    agent_type: AgentType,
    task: String,
) -> Result<ForkedAgentContext, AgentError> {
    let system_msg = ChatMessage {
        role: "system".into(),
        content: shared.system_prompt.clone(),
        tool_call_id: None,
        tool_calls: None,
        reasoning_content: None,
    };
    ForkedAgentContext::fork(
        &system_msg,
        shared.session.id.clone(),
        agent_type,
        task,
        agent_type.max_react_loops_for(&shared.settings.agent),
        std::collections::HashMap::new(),
        false,
    )
    .map_err(|e| {
        if e == ForkError::InvalidMaxReactLoops(0) {
            AgentError::NestedForkProhibited
        } else {
            AgentError::Fork(e)
        }
    })
}

pub fn clear_subagent_queue(shared: &crate::EngineShared) {
    if let Ok(mut guard) = shared.subagent_queue.lock() {
        guard.clear();
    }
}

pub async fn drain_subagent_jobs(
    engine: &mut AgentEngine,
    event_tx: Option<&mpsc::UnboundedSender<Event>>,
) -> Result<(), AgentError> {
    let jobs: Vec<SubagentJob> = {
        let mut guard = engine
            .shared
            .subagent_queue
            .lock()
            .map_err(|_| AgentError::Validation("subagent queue lock poisoned".into()))?;
        std::mem::take(&mut *guard)
            .into_iter()
            .map(SubagentJob::from_pending)
            .collect::<Result<Vec<_>, _>>()?
    };
    if jobs.is_empty() {
        return Ok(());
    }

    let llm_snap = read_session_llm(&engine.shared);
    let hook_batch = jobs
        .iter()
        .any(|j| matches!(j.kind, SubagentJobKind::HookAuditor));
    if hook_batch {
        engine
            .shared
            .audit_log(&novel_logging::LogEvent::KnowledgeAuditorHookForked {
                session_id: engine.shared.session.id.clone(),
                trigger_tool: format!(
                    "batch: {} hook job(s)",
                    jobs.iter()
                        .filter(|j| matches!(j.kind, SubagentJobKind::HookAuditor))
                        .count()
                ),
            });
    }

    let saved_perm = engine.shared.permission_mode_override.clone();
    if hook_batch {
        if let Ok(mut g) = engine.shared.permission_mode_override.lock() {
            *g = PermissionMode::Auto;
        }
    }

    engine
        .shared
        .drain_in_progress
        .store(true, Ordering::SeqCst);
    struct DrainGuard(Arc<std::sync::atomic::AtomicBool>);
    impl Drop for DrainGuard {
        fn drop(&mut self) {
            self.0.store(false, Ordering::SeqCst);
        }
    }
    let _guard = DrainGuard(Arc::clone(&engine.shared.drain_in_progress));

    let mut handles = Vec::with_capacity(jobs.len());
    let mut meta: Vec<(AgentType, String, bool)> = Vec::with_capacity(jobs.len());
    for job in jobs {
        let agent_type = job.agent_type;
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
        meta.push((agent_type, fork_run_id.clone(), inject));
        handles.push(tokio::spawn(async move {
            let result =
                run_subagent_job(shared, job_for_spawn, fork_run_id, snap, event_tx_clone).await;
            (agent_type, result)
        }));
    }

    for (i, handle) in handles.into_iter().enumerate() {
        let (_agent_type, fork_run_id, inject) =
            meta.get(i)
                .cloned()
                .unwrap_or((AgentType::KnowledgeAuditor, String::new(), false));
        match handle.await {
            Ok((at, Ok(output))) => {
                if inject {
                    engine.inject_sub_agent_report(at, &output, Some(&fork_run_id))?;
                    if engine.compaction_needed() {
                        engine.compact_with_events(event_tx).await;
                    }
                } else {
                    let _ = fork_transcript::finish_fork_run(
                        &engine.shared.session.db,
                        &fork_run_id,
                        "complete",
                        None,
                    );
                }
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

    if hook_batch {
        if let Ok(mut g) = saved_perm.lock() {
            *g = PermissionMode::Normal;
        }
    }
    Ok(())
}
