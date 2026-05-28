#![deny(clippy::unwrap_used)]
#![cfg_attr(test, allow(clippy::unwrap_used))]

#[cfg(test)]
mod test_env;

mod agent;
mod context;
mod context_ext;
mod dynamic_context;
mod engine;
mod error;
mod fork;
pub mod fork_transcript;
mod hooks;
mod interrupt;
mod message_bridge;
mod messages;
mod prompt_loader;
mod session;
mod system_prompt;
mod turn;
mod subagent_overflow;
mod subagent_react;
mod turn_loop;
mod types;

pub use agent::{AgentDefinition, AgentType, FORKABLE_AGENT_TYPE_NAMES};
pub use context::ContextManager;
pub use dynamic_context::{
    build_dynamic_context, dedupe_reference_paths, dedupe_skill_ids,
    filter_loadable_reference_paths, filter_loadable_skill_ids,
    format_activated_skill_block, format_invoked_skill_bodies, load_memory, load_progress,
    load_skill_reference_body, parse_skill_reference_path,
};
pub use engine::{AgentEngine, EngineConfig, EngineShared, EngineStatus};
pub use subagent_overflow::{
    build_partial_report, task_preview_120, OVERFLOW_KIND_INPUT_REJECTED,
    OVERFLOW_KIND_OUTPUT_TRUNCATED,
};
pub use turn_loop::run_subagent_async;
pub use error::AgentError;
pub use fork::{ConversationFork, ForkError, ForkedAgentContext};
pub use interrupt::{AbortController, InterruptReason, ERROR_MESSAGE_USER_ABORT};
pub use messages::yield_missing_tool_result_blocks;
pub use prompt_loader::{format_fork_task, load_agent_prompt};
pub use session::SessionHandle;
pub use system_prompt::{DynamicContext, StaticPrompt, SystemPromptBuilder};
pub use turn::{
    TurnContext, MSG_SEQ_APPROVE, MSG_SEQ_CONTINUE, MSG_SEQ_DENY, MSG_SEQ_TOOL_BASE, MSG_SEQ_USER,
};
pub use types::{ChatMessage, CompactionAction, ContentBlockKind, Event, Op, TerminalReason, ToolCallRecord};
