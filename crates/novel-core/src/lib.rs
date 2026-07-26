#![deny(clippy::unwrap_used)]
#![cfg_attr(test, allow(clippy::unwrap_used))]
#![cfg_attr(test, allow(clippy::expect_used))]

#[cfg(test)]
mod test_env;

mod agent;
mod context;
mod engine;
mod error;
mod fork_stream_subs;
mod hooks;
mod interaction_mode;
mod interrupt;
mod message;
mod permission;
mod read_cache;
mod session_todos;
mod subagent;
mod turn;
pub(crate) mod types;

// ── Public API (novel-server / integration tests) ─────────────────
pub use agent::{
    audit_skill_id, fallback_prompt, format_fork_task, load_agent_prompt, system_prompt, AgentType,
    SkillLoadRoots, FORKABLE_AGENT_TYPE_NAMES, FORK_AGENT_CATALOG,
};
pub use context::{DynamicContext, SystemPromptBuilder};
pub use engine::{AgentEngine, EngineConfig, EngineStatus};
pub use error::AgentError;
pub use fork_stream_subs::{
    is_fork_stream_subscribed, new_fork_stream_subscriptions, try_send_fork_overlay_event,
    ForkStreamSubscriptions,
};
pub use hooks::{resolve_tool_visibility, tool_schemas_for_visibility, ToolVisibility};
pub use interaction_mode::InteractionMode;
pub use interrupt::{AbortController, InterruptReason};
pub use message::stored_message_display_text;
pub use permission::{
    format_enter_unattended_prefix, permission_mode_message_kind, prepend_permission_notice,
};
pub use subagent::ForkError;
pub use types::{CompactionAction, ContentBlockKind, Event, Op, TerminalReason};

// Internal ergonomics for `use crate::Type` within this crate.
pub(crate) use agent::AgentDefinition;
pub(crate) use context::ContextManager;
pub(crate) use engine::EngineShared;
pub(crate) use engine::SessionHandle;
pub(crate) use subagent::ForkedAgentContext;
pub(crate) use types::{ChatMessage, ToolCallRecord};
