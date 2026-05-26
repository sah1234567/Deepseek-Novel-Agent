use crate::TerminalReason;
use std::collections::HashMap;

/// Sequence constants for message ordering within a turn.
///
/// User messages use `0`, assistant/tool messages use `1, 2, 3…` via allocation.
/// Mid-turn sub-agent reports keep `role=user` but take the next allocated sequence
/// (never `MSG_SEQ_USER`). Tool results in approval paths use `MSG_SEQ_TOOL_BASE`+.
pub const MSG_SEQ_USER: i32 = 0;
pub const MSG_SEQ_TOOL_BASE: i32 = 900;
pub const MSG_SEQ_DENY: i32 = 910;
pub const MSG_SEQ_APPROVE: i32 = 911;
pub const MSG_SEQ_CONTINUE: i32 = 912;

#[derive(Debug, Clone)]
pub struct PendingToolApproval {
    pub tool_call_id: String,
    pub name: String,
    pub input: serde_json::Value,
}

#[derive(Debug)]
pub struct TurnContext {
    pub turn_number: u32,
    /// Monotonic counter for segment index and LLM iteration tracking.
    pub inner_turn: u32,
    /// Baseline for `inner_turn` when resuming mid-turn (approve/deny/answer).
    /// Budget uses `inner_turn - inner_turn_at_start` so long sessions do not exhaust the cap.
    pub inner_turn_at_start: u32,
    pub max_inner_turns: u32,
    pub pending_approvals: HashMap<String, PendingToolApproval>,
}

impl TurnContext {
    pub fn new(turn_number: u32, max_inner_turns: u32) -> Self {
        Self {
            turn_number,
            inner_turn: 0,
            inner_turn_at_start: 0,
            max_inner_turns,
            pending_approvals: HashMap::new(),
        }
    }

    pub fn inner_spent(&self) -> u32 {
        self.inner_turn.saturating_sub(self.inner_turn_at_start)
    }

    pub fn needs_continuation(&self) -> bool {
        self.inner_spent() < self.max_inner_turns
    }

    pub fn increment_inner(&mut self) -> Result<(), TerminalReason> {
        self.inner_turn += 1;
        if self.inner_spent() >= self.max_inner_turns {
            return Err(TerminalReason::MaxReactLoops(self.max_inner_turns));
        }
        Ok(())
    }

    pub fn has_pending_approvals(&self) -> bool {
        !self.pending_approvals.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn max_react_loops_reached() {
        let mut t = TurnContext::new(1, 2);
        assert!(t.needs_continuation());
        assert!(t.increment_inner().is_ok());
        assert!(t.increment_inner().is_err());
    }

    #[test]
    fn resume_mid_turn_does_not_exhaust_budget() {
        let mut t = TurnContext::new(1, 80);
        t.inner_turn = 120;
        t.inner_turn_at_start = 120;
        assert!(t.needs_continuation());
        assert!(t.increment_inner().is_ok());
        assert_eq!(t.inner_turn, 121);
    }
}
