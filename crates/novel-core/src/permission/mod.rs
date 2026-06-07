//! Mid-session permission mode user-message prefixes.

mod mode_prompt;

pub use mode_prompt::{
    format_enter_unattended_prefix, format_mode_transition_prefix, is_permission_mode_notice,
    permission_mode_message_kind, prepend_permission_notice, system_contains_autonomous,
    PERMISSION_MODE_EXIT_PREFIX,
};
