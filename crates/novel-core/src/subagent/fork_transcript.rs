//! Sub-agent transcript persistence (`fork_runs` / `fork_messages`).
//!
//! Isolated from parent session `messages`: never merged into `self.messages` or main LLM prompts.
//! ForkSubAgent tool jobs and PostToolUse hook jobs share this transcript store (UI replay only).

use crate::message::chat_to_json;
use crate::{AgentError, ChatMessage};
use novel_state::Database;

pub fn create_fork_run(
    db: &Database,
    session_id: &str,
    parent_turn_number: i32,
    agent_type: &str,
    task: &str,
    source: &str,
) -> Result<String, AgentError> {
    db.create_fork_run(session_id, parent_turn_number, agent_type, task, source)
        .map_err(AgentError::State)
}

pub fn persist_fork_message(
    db: &Database,
    run_id: &str,
    msg: &ChatMessage,
) -> Result<(), AgentError> {
    db.insert_fork_message(run_id, &msg.role, &chat_to_json(msg))
        .map_err(AgentError::State)?;
    Ok(())
}

pub fn finish_fork_run(
    db: &Database,
    run_id: &str,
    status: &str,
    report_message_id: Option<&str>,
) -> Result<(), AgentError> {
    db.finish_fork_run(run_id, status, report_message_id)
        .map_err(AgentError::State)
}
