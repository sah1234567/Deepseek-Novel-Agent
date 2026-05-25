mod error;
mod level1;
mod level2;
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
pub use level1::{apply_level1_messages, apply_level1_on_compaction_messages};
pub use level2::apply_level2_knowledge;
pub use message_format::format_for_summary;
pub use message_types::{CompactionMessage, CompactionToolCall};
pub use react_cycles::{is_user_turn_start, partition_messages, user_turn_ranges, PartitionResult, SKILL_USER_PREFIX, SUMMARY_USER_PREFIX};
pub use retain_policy::RetainPolicy;
pub use session_rebuild::{
    apply_level4_compaction, rebuild_session_messages, rule_based_summary, wrap_skill_user_message,
    wrap_summary_user_message, SessionRebuildInput,
};
pub use strategy::{CompactionDecision, CompactionStrategy};
pub use summarizer::{build_summary_trailing_user_prompt, truncate_summary};
pub use token_counter::estimate_tokens;
