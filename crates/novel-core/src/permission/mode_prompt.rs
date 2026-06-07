//! Mid-session permission toggles: prepend enter/exit copy to the **next** user message
//! (single `role=user` row). Session boundary Unattended still uses `autonomous-writing.md`
//! in system — see `SystemPromptBuilder`.

use novel_tools::PermissionMode;

pub const PERMISSION_MODE_ENTER_PREFIX: &str = "[权限模式: 无人值守]";
pub const PERMISSION_MODE_EXIT_PREFIX: &str = "[权限模式: 已退出无人值守]";

/// Separator between injected prefix block and author content in a merged user message.
pub const USER_CONTENT_SEPARATOR: &str = "\n\n---\n\n";

/// Substring from `prompt/autonomous-writing.md` — detect rules already in system.
pub const AUTONOMOUS_MODE_MARKER: &str = "自主连续写作模式";

const PERMISSION_MODE_ENTER_HEADER: &str =
    include_str!("../../../../prompt/permission-mode-enter.md");
const PERMISSION_MODE_EXIT_BODY: &str = include_str!("../../../../prompt/permission-mode-exit.md");

pub(crate) fn autonomous_writing_body() -> &'static str {
    include_str!("../../../../prompt/autonomous-writing.md")
}

pub fn system_contains_autonomous(system_content: &str) -> bool {
    system_content.contains(AUTONOMOUS_MODE_MARKER)
}

/// Prefix block prepended to the next user message when entering Unattended mid-session.
pub fn format_enter_unattended_prefix() -> String {
    format!(
        "{}\n\n{}",
        PERMISSION_MODE_ENTER_HEADER.trim(),
        autonomous_writing_body().trim()
    )
}

/// Prefix block prepended to the next user message when leaving Unattended.
pub fn format_exit_unattended_prefix() -> String {
    PERMISSION_MODE_EXIT_BODY.trim().to_string()
}

/// Mid-session mode transition: prefix for the next user message, if any.
pub fn format_mode_transition_prefix(
    old_mode: &PermissionMode,
    new_mode: &PermissionMode,
    system_has_autonomous: bool,
) -> Option<String> {
    if matches!(new_mode, PermissionMode::Unattended)
        && !matches!(old_mode, PermissionMode::Unattended)
    {
        if system_has_autonomous {
            None
        } else {
            Some(format_enter_unattended_prefix())
        }
    } else if matches!(old_mode, PermissionMode::Unattended)
        && !matches!(new_mode, PermissionMode::Unattended)
    {
        Some(format_exit_unattended_prefix())
    } else {
        None
    }
}

/// Merge a pending permission notice with the author's message (one user turn).
pub fn prepend_permission_notice(prefix_block: &str, user_content: &str) -> String {
    format!(
        "{}{}{}",
        prefix_block.trim(),
        USER_CONTENT_SEPARATOR,
        user_content.trim()
    )
}

pub fn is_permission_mode_notice(content: &str) -> bool {
    content.starts_with(PERMISSION_MODE_ENTER_PREFIX)
        || content.starts_with(PERMISSION_MODE_EXIT_PREFIX)
}

/// UI `message_kind` for stored user rows with permission-mode injection.
pub fn permission_mode_message_kind(content: &str) -> Option<&'static str> {
    if content.starts_with(PERMISSION_MODE_ENTER_PREFIX) {
        Some("permissionModeEnter")
    } else if content.starts_with(PERMISSION_MODE_EXIT_PREFIX) {
        Some("permissionModeExit")
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn enter_prefix_contains_header_and_autonomous_body() {
        let prefix = format_enter_unattended_prefix();
        assert!(prefix.starts_with(PERMISSION_MODE_ENTER_PREFIX));
        assert!(prefix.contains(AUTONOMOUS_MODE_MARKER));
        assert!(prefix.contains("审计降频"));
        assert!(!prefix.contains(PERMISSION_MODE_EXIT_PREFIX));
    }

    #[test]
    fn exit_prefix_loaded_from_file() {
        let prefix = format_exit_unattended_prefix();
        assert!(prefix.starts_with(PERMISSION_MODE_EXIT_PREFIX));
        assert!(prefix.contains("AskUserQuestion"));
        assert!(!prefix.contains(AUTONOMOUS_MODE_MARKER));
    }

    #[test]
    fn prepend_merges_single_user_message() {
        let merged = prepend_permission_notice("[权限模式: 无人值守]\nintro", "继续写第 5 章");
        assert!(merged.starts_with("[权限模式: 无人值守]"));
        assert!(merged.contains("---"));
        assert!(merged.ends_with("继续写第 5 章"));
        assert!(!merged.contains("\n\n---\n\n---\n\n"));
    }

    #[test]
    fn is_permission_mode_notice_detects_prefixes() {
        assert!(is_permission_mode_notice("[权限模式: 无人值守]\nbody"));
        assert!(is_permission_mode_notice(
            "[权限模式: 已退出无人值守]\nbody"
        ));
        assert!(!is_permission_mode_notice("hello"));
    }

    #[test]
    fn format_mode_transition_prefix_covers_transitions() {
        use PermissionMode::{Auto, Normal, Plan, Unattended};
        assert!(format_mode_transition_prefix(&Normal, &Unattended, false).is_some());
        assert!(format_mode_transition_prefix(&Unattended, &Auto, false).is_some());
        assert!(format_mode_transition_prefix(&Unattended, &Plan, true).is_some());
        assert!(format_mode_transition_prefix(&Normal, &Unattended, true).is_none());
        assert!(format_mode_transition_prefix(&Auto, &Plan, false).is_none());
    }

    #[test]
    fn permission_mode_message_kind_values() {
        assert_eq!(
            permission_mode_message_kind(PERMISSION_MODE_ENTER_PREFIX),
            Some("permissionModeEnter")
        );
        assert_eq!(
            permission_mode_message_kind(PERMISSION_MODE_EXIT_PREFIX),
            Some("permissionModeExit")
        );
        assert_eq!(permission_mode_message_kind("hello"), None);
    }
}
