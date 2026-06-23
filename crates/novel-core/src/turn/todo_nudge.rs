//! Hidden-user nudge when unfinished session todos remain at inner-turn exit.

use crate::TerminalReason;
use novel_deepseek::LlmCompletion;
use novel_state::SessionTodo;

pub(crate) const REASONING_ONLY_NUDGE: &str =
    "你只输出了思考过程，没有产生实质性内容或调用工具。请继续完成当前任务——产生文字输出或执行必要的工具调用。";

pub(crate) const MAX_TODO_NUDGES: u32 = 3;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum NoToolsCompletionAction {
    EarlyExit(TerminalReason),
    InjectReasoningNudge,
    InjectTodoNudge,
    Complete,
}

pub(crate) fn no_tools_completion_action(
    interrupt_requested: bool,
    pending_tool_count: usize,
    completion: &LlmCompletion,
    unfinished_todo_count: usize,
    todo_nudge_count: u32,
) -> NoToolsCompletionAction {
    if let Some(reason) = inner_turn_early_exit_reason(interrupt_requested, pending_tool_count) {
        return NoToolsCompletionAction::EarlyExit(reason);
    }
    if crate::turn::llm_stream::should_continue_inner_after_completion(completion) {
        return NoToolsCompletionAction::InjectReasoningNudge;
    }
    if todo_nudge_count < MAX_TODO_NUDGES && unfinished_todo_count > 0 {
        return NoToolsCompletionAction::InjectTodoNudge;
    }
    NoToolsCompletionAction::Complete
}

pub(crate) fn unfinished_todo_nudge_message(unfinished: &[SessionTodo]) -> Option<String> {
    if unfinished.is_empty() {
        return None;
    }
    let lines: Vec<String> = unfinished
        .iter()
        .map(|t| {
            let status_label = match t.status.as_str() {
                "in_progress" => "进行中",
                _ => "待处理",
            };
            format!("- [{}] {}", status_label, t.content)
        })
        .collect();
    Some(format!(
        "你还有 {} 个未完成的任务，本轮即将结束前必须全部清零：\n{}\n\n请按以下步骤处理：\n1. **检查**：逐一对照上述任务，回顾本轮实际操作，判断每项是否已真实完成；\n2. **标记**：已完成的立即调用 TodoWrite 标记为 completed，不再需要的标记为 cancelled；\n3. **继续**：仍未完成的任务继续执行，不得跳过。\n\n全部任务为 completed 或 cancelled 后本轮方可正常结束。",
        unfinished.len(),
        lines.join("\n")
    ))
}

pub(crate) fn inner_turn_early_exit_reason(
    interrupt_requested: bool,
    pending_tool_count: usize,
) -> Option<crate::TerminalReason> {
    if interrupt_requested {
        return Some(crate::TerminalReason::AbortedStreaming);
    }
    if pending_tool_count > 0 {
        return Some(crate::TerminalReason::Completed);
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use novel_deepseek::LlmCompletion;

    fn completion(content: Option<&str>, reasoning: Option<&str>) -> LlmCompletion {
        LlmCompletion {
            content: content.map(str::to_string),
            reasoning_content: reasoning.map(str::to_string),
            tool_calls: vec![],
            stop_reason: Some("stop".into()),
            usage: None,
        }
    }

    #[test]
    fn nudge_lists_unfinished_only() {
        let msg = unfinished_todo_nudge_message(&[SessionTodo {
            id: "1".into(),
            content: "写细纲".into(),
            status: "in_progress".into(),
        }])
        .unwrap();
        assert!(msg.contains("写细纲"));
        assert!(msg.contains("进行中"));
    }

    #[test]
    fn early_exit_on_interrupt() {
        assert!(matches!(
            inner_turn_early_exit_reason(true, 0),
            Some(crate::TerminalReason::AbortedStreaming)
        ));
    }

    #[test]
    fn early_exit_when_pending_tools() {
        assert!(matches!(
            inner_turn_early_exit_reason(false, 2),
            Some(crate::TerminalReason::Completed)
        ));
    }

    #[test]
    fn reasoning_only_completion_nudges_continue() {
        let c = completion(None, Some("thinking"));
        assert!(matches!(
            no_tools_completion_action(false, 0, &c, 0, 0),
            NoToolsCompletionAction::InjectReasoningNudge
        ));
    }

    #[test]
    fn unfinished_todos_trigger_nudge_before_complete() {
        let c = completion(Some("done"), None);
        assert!(matches!(
            no_tools_completion_action(false, 0, &c, 2, 0),
            NoToolsCompletionAction::InjectTodoNudge
        ));
    }

    #[test]
    fn max_todo_nudges_allows_complete() {
        let c = completion(Some("done"), None);
        assert!(matches!(
            no_tools_completion_action(false, 0, &c, 2, MAX_TODO_NUDGES),
            NoToolsCompletionAction::Complete
        ));
    }
}
