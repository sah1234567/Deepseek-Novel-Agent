//! Mid-session permission mode user-message prefixes.

mod mode_prompt;
mod resolve;
mod transition;

#[cfg(test)]
pub use mode_prompt::PERMISSION_MODE_ENTER_PREFIX;
pub use mode_prompt::{
    format_enter_unattended_prefix, is_permission_mode_notice, permission_mode_message_kind,
    prepend_permission_notice, system_contains_autonomous, PERMISSION_MODE_EXIT_PREFIX,
};
#[cfg(test)]
pub use mode_prompt::{AUTONOMOUS_MODE_MARKER, USER_CONTENT_SEPARATOR};
pub use resolve::resolve_session_permission_mode;
pub use transition::{plan_mode_transition, ModeTransitionPlan};
