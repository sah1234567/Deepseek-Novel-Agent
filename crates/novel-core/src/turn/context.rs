use crate::TerminalReason;

/// User message sequence within a turn (`turn_number ≥ 1`).
///
/// Assistant/tool rows use `1, 2, 3…` via `alloc_turn_message_seq`.
/// Mid-turn sub-agent reports keep `role=user` but take the next allocated sequence
/// (never `MSG_SEQ_USER`).
pub const MSG_SEQ_USER: i32 = 0;

/// Mid-turn sub-agent report user messages (`role=user`, not a new turn).
pub const SUB_AGENT_REPORT_PREFIX: &str = "[子 Agent 完成:";

#[derive(Debug)]
pub struct TurnContext {
    /// Monotonic counter for segment index and LLM iteration tracking.
    pub inner_turn: u32,
    /// Baseline for `inner_turn` when resuming mid-turn (approve/deny/answer).
    /// Budget uses `inner_turn - inner_turn_at_start` so long sessions do not exhaust the cap.
    pub inner_turn_at_start: u32,
    pub max_inner_turns: u32,
}

impl TurnContext {
    pub fn new(max_inner_turns: u32) -> Self {
        Self {
            inner_turn: 0,
            inner_turn_at_start: 0,
            max_inner_turns,
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
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn max_react_loops_reached() {
        let mut t = TurnContext::new(2);
        assert!(t.needs_continuation());
        assert!(t.increment_inner().is_ok());
        assert!(t.increment_inner().is_err());
    }

    #[test]
    fn resume_mid_turn_does_not_exhaust_budget() {
        let mut t = TurnContext::new(80);
        t.inner_turn = 120;
        t.inner_turn_at_start = 120;
        assert!(t.needs_continuation());
        assert!(t.increment_inner().is_ok());
        assert_eq!(t.inner_turn, 121);
    }
}
