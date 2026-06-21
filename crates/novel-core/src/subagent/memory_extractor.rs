//! Background extractMemories fork — fire-and-forget after each completed turn.

use crate::engine::session_llm::SessionLlmSnapshot;
use crate::subagent::runner::run_subagent_job_with_child;
use crate::subagent::{build_fork_child, SubagentJob, SubagentJobKind};
use crate::{AgentError, AgentType, ChatMessage};
use novel_memory::{MemoryExtractor, PreparedMemoryExtraction};
use std::sync::Arc;

/// Spawn memory extraction in the background (does not block the main turn).
pub fn spawn_memory_extraction(
    shared: crate::EngineShared,
    extractor: Arc<MemoryExtractor>,
    prepared: PreparedMemoryExtraction,
    recent_messages: Vec<ChatMessage>,
    llm_snap: SessionLlmSnapshot,
    all_messages: Arc<Vec<ChatMessage>>,
) {
    tokio::spawn(async move {
        shared.sub_agent_inc();
        let result = run_memory_extraction_once(
            shared.clone(),
            &prepared,
            recent_messages,
            llm_snap.clone(),
        )
        .await;
        if let Err(e) = result {
            tracing::warn!(error = %e, "memory_extraction_failed");
            shared.sub_agent_dec();
        }
        if let Some(trailing) = extractor.complete_extraction(prepared.message_count) {
            let cursor = extractor.cursor();
            let end = trailing.message_count.min(all_messages.len());
            let recent = if cursor < end {
                all_messages[cursor..end].to_vec()
            } else {
                Vec::new()
            };
            spawn_memory_extraction(shared, extractor, trailing, recent, llm_snap, all_messages);
        }
    });
}

async fn run_memory_extraction_once(
    shared: crate::EngineShared,
    prepared: &PreparedMemoryExtraction,
    recent_messages: Vec<ChatMessage>,
    llm_snap: SessionLlmSnapshot,
) -> Result<(), AgentError> {
    let mut child = build_fork_child(
        &shared,
        AgentType::MemoryExtractor,
        prepared.task_prompt.clone(),
    )?;
    child.messages.extend(recent_messages);

    let job = SubagentJob {
        agent_type: AgentType::MemoryExtractor,
        task: prepared.task_prompt.clone(),
        kind: SubagentJobKind::MemoryExtraction,
    };

    tracing::debug!(
        session_id = %shared.session.id,
        message_count = prepared.message_count,
        context_messages = child.messages.len(),
        "memory_extraction_spawned"
    );

    let _ = run_subagent_job_with_child(shared, job, String::new(), llm_snap, None, Some(child))
        .await?;

    Ok(())
}
