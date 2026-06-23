//! Bundled argument structs for subagent ReAct / LLM / tool paths.

use crate::engine::session_llm::SessionLlmSnapshot;
use crate::fork_stream_subs::ForkStreamSubscriptions;
use crate::turn::TurnContext;
use crate::{AgentType, ChatMessage, EngineShared, Event};
use novel_deepseek::{LlmChatMessage, LlmCompletion, LlmToolCall};
use novel_state::Database;
use novel_tools::ToolRegistry;
use tokio::sync::mpsc;

/// Per-job context threaded through the subagent ReAct loop (`runner.rs`).
pub(crate) struct SubagentJobCtx<'a> {
    pub shared: &'a EngineShared,
    pub agent_type: AgentType,
    pub task: &'a str,
    pub fork_run_id: &'a str,
    pub schemas: &'a [(String, String, serde_json::Value)],
    pub llm_snap: &'a SessionLlmSnapshot,
    pub event_tx: Option<&'a mpsc::UnboundedSender<Event>>,
    pub max_react_loops: u32,
}

/// Inputs for one subagent LLM fetch (`llm_turn.rs`).
pub(crate) struct SubagentLlmFetchParams<'a> {
    pub shared: &'a EngineShared,
    pub agent_type: AgentType,
    pub task: &'a str,
    pub fork_run_id: &'a str,
    pub llm_msgs: &'a [LlmChatMessage],
    pub active_schemas: &'a [(String, String, serde_json::Value)],
    pub llm_snap: &'a SessionLlmSnapshot,
    pub event_tx: Option<&'a mpsc::UnboundedSender<Event>>,
    pub message_snapshot: &'a [ChatMessage],
    pub inner_turn: u32,
}

impl<'a> SubagentLlmFetchParams<'a> {
    pub(crate) fn completion_params(&self) -> SubagentCompletionParams<'a> {
        SubagentCompletionParams {
            shared: self.shared,
            agent_type: self.agent_type,
            task: self.task,
            fork_run_id: self.fork_run_id,
            llm_snap: self.llm_snap,
            event_tx: self.event_tx,
            inner_turn: self.inner_turn,
        }
    }
}

impl<'a> SubagentJobCtx<'a> {
    pub(crate) fn llm_fetch_params(
        &self,
        llm_msgs: &'a [LlmChatMessage],
        active_schemas: &'a [(String, String, serde_json::Value)],
        message_snapshot: &'a [ChatMessage],
        inner_turn: u32,
    ) -> SubagentLlmFetchParams<'a> {
        SubagentLlmFetchParams {
            shared: self.shared,
            agent_type: self.agent_type,
            task: self.task,
            fork_run_id: self.fork_run_id,
            llm_msgs,
            active_schemas,
            llm_snap: self.llm_snap,
            event_tx: self.event_tx,
            message_snapshot,
            inner_turn,
        }
    }
}

/// Post-completion handling after a subagent LLM segment (`llm_turn.rs`).
pub(crate) struct SubagentCompletionParams<'a> {
    pub shared: &'a EngineShared,
    pub agent_type: AgentType,
    pub task: &'a str,
    pub fork_run_id: &'a str,
    pub llm_snap: &'a SessionLlmSnapshot,
    pub event_tx: Option<&'a mpsc::UnboundedSender<Event>>,
    pub inner_turn: u32,
}

/// Persist tool results into fork transcript (`helpers.rs`).
pub(crate) struct SubagentToolResultsParams<'a> {
    pub registry: &'a ToolRegistry,
    pub db: &'a Database,
    pub fork_run_id: &'a str,
    pub agent_type: AgentType,
    pub turn_ctx: &'a TurnContext,
    pub tool_calls: &'a [LlmToolCall],
    pub completion: &'a LlmCompletion,
    pub event_tx: Option<&'a mpsc::UnboundedSender<Event>>,
    pub subs: &'a ForkStreamSubscriptions,
}
