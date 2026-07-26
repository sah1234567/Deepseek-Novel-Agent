//! Mid-session permission toggles: prepend enter/exit copy to the **next** user message
//! (single `role=user` row). Loads autonomous-writing rules from the skill file for
//! mid-session injection; new sessions use InvokeSkill hint in system prompt.

pub const PERMISSION_MODE_ENTER_PREFIX: &str = "[权限模式: 无人值守]";
pub const PERMISSION_MODE_EXIT_PREFIX: &str = "[权限模式: 已退出无人值守]";

/// Separator between injected prefix block and author content in a merged user message.
pub const USER_CONTENT_SEPARATOR: &str = "\n\n---\n\n";

/// Substring from autonomous-writing skill — detect rules already in system.
pub const AUTONOMOUS_MODE_MARKER: &str = "自主连续写作模式";

const PERMISSION_MODE_ENTER_HEADER: &str =
    include_str!("../../../../prompt/permission-mode-enter.md");
const PERMISSION_MODE_EXIT_BODY: &str = include_str!("../../../../prompt/permission-mode-exit.md");
const AUTONOMOUS_WRITING_SKILL_RAW: &str =
    include_str!("../../../../skills/autonomous-writing/SKILL.md");

/// Drop leading YAML frontmatter (`---` … `---`) from a skill markdown body.
fn strip_yaml_frontmatter(raw: &str) -> &str {
    let trimmed = raw.trim_start();
    if !trimmed.starts_with("---") {
        return raw;
    }
    let after_open = &trimmed[3..];
    let Some(close_rel) = after_open.find("\n---") else {
        return raw;
    };
    let after_close = &after_open[close_rel + 4..];
    after_close
        .trim_start_matches('\r')
        .trim_start_matches('\n')
}

pub(crate) fn autonomous_writing_body() -> &'static str {
    strip_yaml_frontmatter(AUTONOMOUS_WRITING_SKILL_RAW)
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

/// `Some(true)` = enter notice, `Some(false)` = exit, `None` = unrelated user text.
pub(crate) fn permission_notice_direction(content: &str) -> Option<bool> {
    if content.starts_with(PERMISSION_MODE_ENTER_PREFIX) {
        Some(true)
    } else if content.starts_with(PERMISSION_MODE_EXIT_PREFIX) {
        Some(false)
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
    permission_notice_direction(content).is_some()
}

/// UI `message_kind` for stored user rows with permission-mode injection.
pub fn permission_mode_message_kind(content: &str) -> Option<&'static str> {
    match permission_notice_direction(content) {
        Some(true) => Some("permissionModeEnter"),
        Some(false) => Some("permissionModeExit"),
        None => None,
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
        assert!(
            !prefix.contains("name: autonomous-writing"),
            "frontmatter must be stripped"
        );
        assert!(
            !prefix.contains("skill_kind:"),
            "frontmatter must be stripped"
        );
    }

    #[test]
    fn autonomous_body_strips_yaml_frontmatter() {
        let body = autonomous_writing_body();
        assert!(body.starts_with("# 自主连续写作模式") || body.contains("# 自主连续写作模式"));
        assert!(!body.contains("allowed-tools:"));
        assert!(!body.starts_with("---"));
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
