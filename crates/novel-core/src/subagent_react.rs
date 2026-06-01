//! Subagent ReAct budget helpers (distinct from Session Turn / `turn_number`).

use crate::ChatMessage;

pub const REPORT_GRACE_LOOPS: u32 = 1;

/// Injected when ReAct budget is exhausted; triggers a final report-only LLM round.
pub fn react_limit_reminder_message(spent: u32, max: u32) -> ChatMessage {
    ChatMessage {
        role: "user".into(),
        content: format!(
            "<system-reminder>\n\
             ReAct 循环已达上限 ({spent}/{max})。禁止再调用任何工具。\n\
             请立即基于本轮已收集的全部 tool 结果，输出完整自然语言报告（含 ## 接下来（主 Agent 必读））。\n\
             </system-reminder>"
        ),
        tool_call_id: None,
        tool_calls: None,
        reasoning_content: None,
    }
}

pub fn report_only_tool_rejection(tool_call_id: &str) -> ChatMessage {
    ChatMessage {
        role: "tool".into(),
        content: "Error: ReAct 预算已用尽，禁止调用工具。请直接输出报告文本，不要 tool call。"
            .into(),
        tool_call_id: Some(tool_call_id.to_string()),
        tool_calls: None,
        reasoning_content: None,
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SubagentLoopPhase {
    Reacting,
    ReportOnly { grace_left: u32 },
}

impl SubagentLoopPhase {
    pub fn enter_report_only(self) -> Self {
        Self::ReportOnly {
            grace_left: REPORT_GRACE_LOOPS,
        }
    }

    pub fn is_report_only(self) -> bool {
        matches!(self, Self::ReportOnly { .. })
    }

    pub fn consume_grace(self) -> Option<Self> {
        match self {
            Self::Reacting => None,
            Self::ReportOnly { grace_left: 0 } => None,
            Self::ReportOnly { grace_left: 1 } => Some(Self::ReportOnly { grace_left: 0 }),
            Self::ReportOnly { grace_left: n } => Some(Self::ReportOnly { grace_left: n - 1 }),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reminder_mentions_limit() {
        let m = react_limit_reminder_message(40, 40);
        assert!(m.content.contains("40/40"));
        assert!(m.content.contains("禁止再调用"));
    }
}
