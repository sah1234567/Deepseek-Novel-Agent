//! Mid-session permission mode transition planning (Dual / Single / None injection).

use novel_tools::PermissionMode;

use super::mode_prompt::{format_enter_unattended_prefix, format_exit_unattended_prefix};

/// Planned user-message prefix for the next turn after a permission mode switch.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ModeTransitionPlan {
    /// Enter header + autonomous body merged into one prefix block.
    DualEnterUnattended { merged_prefix: String },
    /// Exit unattended notice.
    ExitUnattended { prefix: String },
    /// No user-message prefix (tool policy only, or rules already in system).
    None,
}

impl ModeTransitionPlan {
    pub fn merged_prefix(&self) -> Option<&str> {
        match self {
            Self::DualEnterUnattended { merged_prefix } => Some(merged_prefix.as_str()),
            Self::ExitUnattended { prefix } => Some(prefix.as_str()),
            Self::None => None,
        }
    }
}

fn entering_unattended(old_mode: &PermissionMode, new_mode: &PermissionMode) -> bool {
    matches!(new_mode, PermissionMode::Unattended)
        && !matches!(old_mode, PermissionMode::Unattended)
}

fn leaving_unattended(old_mode: &PermissionMode, new_mode: &PermissionMode) -> bool {
    matches!(old_mode, PermissionMode::Unattended)
        && !matches!(new_mode, PermissionMode::Unattended)
}

/// Decide injection for `old_mode` → `new_mode`. Caller must reject `old == new` before calling.
pub fn plan_mode_transition(
    old_mode: &PermissionMode,
    new_mode: &PermissionMode,
    system_has_autonomous: bool,
) -> ModeTransitionPlan {
    debug_assert!(old_mode != new_mode);

    if entering_unattended(old_mode, new_mode) {
        return if system_has_autonomous {
            ModeTransitionPlan::None
        } else {
            ModeTransitionPlan::DualEnterUnattended {
                merged_prefix: format_enter_unattended_prefix(),
            }
        };
    }
    if leaving_unattended(old_mode, new_mode) {
        return ModeTransitionPlan::ExitUnattended {
            prefix: format_exit_unattended_prefix(),
        };
    }
    ModeTransitionPlan::None
}

#[cfg(test)]
mod tests {
    use super::*;
    use rstest::rstest;

    #[rstest]
    #[case(
        PermissionMode::Normal,
        PermissionMode::Plan,
        false,
        ModeTransitionPlan::None
    )]
    #[case(
        PermissionMode::Normal,
        PermissionMode::Auto,
        false,
        ModeTransitionPlan::None
    )]
    #[case(
        PermissionMode::Plan,
        PermissionMode::Auto,
        false,
        ModeTransitionPlan::None
    )]
    #[case(
        PermissionMode::Auto,
        PermissionMode::Normal,
        false,
        ModeTransitionPlan::None
    )]
    #[case(
        PermissionMode::Plan,
        PermissionMode::Normal,
        false,
        ModeTransitionPlan::None
    )]
    fn non_unattended_transitions_are_none(
        #[case] old: PermissionMode,
        #[case] new: PermissionMode,
        #[case] system_has_autonomous: bool,
        #[case] expected: ModeTransitionPlan,
    ) {
        assert_eq!(
            plan_mode_transition(&old, &new, system_has_autonomous),
            expected
        );
    }

    #[rstest]
    #[case(PermissionMode::Normal)]
    #[case(PermissionMode::Plan)]
    #[case(PermissionMode::Auto)]
    fn enter_unattended_dual_when_system_lacks_autonomous(#[case] old: PermissionMode) {
        let plan = plan_mode_transition(&old, &PermissionMode::Unattended, false);
        assert!(matches!(
            plan,
            ModeTransitionPlan::DualEnterUnattended { .. }
        ));
        let prefix = plan.merged_prefix().unwrap();
        assert!(prefix.contains(super::super::mode_prompt::PERMISSION_MODE_ENTER_PREFIX));
        assert!(prefix.contains(super::super::mode_prompt::AUTONOMOUS_MODE_MARKER));
    }

    #[rstest]
    #[case(PermissionMode::Normal)]
    #[case(PermissionMode::Plan)]
    #[case(PermissionMode::Auto)]
    fn enter_unattended_none_when_system_has_autonomous(#[case] old: PermissionMode) {
        assert_eq!(
            plan_mode_transition(&old, &PermissionMode::Unattended, true),
            ModeTransitionPlan::None
        );
    }

    #[rstest]
    #[case(PermissionMode::Normal)]
    #[case(PermissionMode::Plan)]
    #[case(PermissionMode::Auto)]
    fn exit_unattended_single(#[case] new: PermissionMode) {
        let plan = plan_mode_transition(&PermissionMode::Unattended, &new, false);
        assert!(matches!(plan, ModeTransitionPlan::ExitUnattended { .. }));
        let prefix = plan.merged_prefix().unwrap();
        assert!(prefix.starts_with(super::super::mode_prompt::PERMISSION_MODE_EXIT_PREFIX));
    }
}
