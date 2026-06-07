#![deny(clippy::unwrap_used)]
#![cfg_attr(test, allow(clippy::unwrap_used))]

#[cfg(test)]
mod test_env;

mod agent;
mod context;
mod engine;
mod error;
mod hooks;
mod interrupt;
mod message;
mod permission;
mod subagent;
mod turn;
pub(crate) mod types;

// ── Public API (novel-server / integration tests) ─────────────────
pub use agent::{AgentType, FORKABLE_AGENT_TYPE_NAMES};
pub use engine::{AgentEngine, EngineConfig, EngineStatus};
pub use error::AgentError;
pub use interrupt::{AbortController, InterruptReason};
pub use message::stored_message_display_text;
pub use permission::{
    format_enter_unattended_prefix, permission_mode_message_kind, prepend_permission_notice,
};
pub use subagent::ForkError;
pub use types::{CompactionAction, ContentBlockKind, Event, Op, TerminalReason};

// Internal ergonomics for `use crate::Type` within this crate.
pub(crate) use agent::AgentDefinition;
pub(crate) use context::{ContextManager, DynamicContext, SystemPromptBuilder};
pub(crate) use engine::EngineShared;
pub(crate) use engine::SessionHandle;
pub(crate) use subagent::ForkedAgentContext;
pub(crate) use types::{ChatMessage, ToolCallRecord};
