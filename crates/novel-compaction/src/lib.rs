#![deny(clippy::unwrap_used)]
#![cfg_attr(test, allow(clippy::unwrap_used))]

mod error;
mod message_format;
mod message_types;
mod react_cycles;
mod retain_policy;
mod session_rebuild;
mod strategy;
mod summarizer;
mod token_counter;

/// Role + content pair.
pub type RoleContent = (String, String);

pub use error::CompactionError;
pub use message_format::format_for_summary;
pub use message_types::{CompactionMessage, CompactionToolCall};
pub use react_cycles::{
    is_user_turn_start, partition_messages, user_turn_ranges, PartitionResult,
    CONTEXT_REFRESH_USER_PREFIX,
};
pub use retain_policy::RetainPolicy;
pub use session_rebuild::{
    rebuild_session_messages, rebuild_session_under_budget, rule_based_summary,
    wrap_context_refresh_user_message, SessionBudgetRebuildInput, SessionRebuildInput,
};
pub use strategy::{CompactionDecision, CompactionStrategy};
pub use summarizer::{build_summary_trailing_user_prompt, truncate_summary};
pub use token_counter::estimate_tokens;
