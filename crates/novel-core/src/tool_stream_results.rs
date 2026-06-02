//! Tool stream result merging and ordering (extracted from turn_loop for testability).

use novel_tools::{ToolError, ToolOutput};
use std::collections::HashMap;

/// Merge streaming tool results by `tool_call_id`. `NeedsUserInput` wins over `Ok`; `Ok` can replace `Err`.
pub(crate) fn merge_stream_results_by_id(
    results: Vec<(String, Result<ToolOutput, ToolError>)>,
) -> HashMap<String, Result<ToolOutput, ToolError>> {
    let mut by_id: HashMap<String, Result<ToolOutput, ToolError>> = HashMap::new();
    for (id, result) in results {
        match by_id.get(&id) {
            None => {
                by_id.insert(id, result);
            }
            Some(existing) => {
                let incoming_needs_input = matches!(&result, Err(ToolError::NeedsUserInput { .. }));
                let existing_needs_input =
                    matches!(existing, Err(ToolError::NeedsUserInput { .. }));
                if incoming_needs_input {
                    by_id.insert(id, result);
                } else if existing_needs_input {
                    // Keep AskUserQuestion pause signal.
                } else if result.is_ok() && existing.is_err() {
                    by_id.insert(id, result);
                }
            }
        }
    }
    by_id
}

/// Order tool result ids: `tool_call_order` first, then any remaining keys from `by_id`.
pub(crate) fn ordered_tool_result_ids(
    tool_call_order: &[String],
    by_id: &HashMap<String, Result<ToolOutput, ToolError>>,
) -> Vec<String> {
    let mut ordered_ids: Vec<String> = tool_call_order
        .iter()
        .filter(|id| by_id.contains_key(*id))
        .cloned()
        .collect();
    for id in by_id.keys() {
        if !ordered_ids.iter().any(|o| o == id) {
            ordered_ids.push(id.clone());
        }
    }
    ordered_ids
}

#[cfg(test)]
mod tests {
    use super::*;
    use novel_tools::ToolOutput;

    fn ok_out() -> Result<ToolOutput, ToolError> {
        Ok(ToolOutput {
            content: "ok".into(),
            is_error: false,
        })
    }

    fn needs_input() -> Result<ToolOutput, ToolError> {
        Err(ToolError::NeedsUserInput {
            payload: novel_tools::AskUserQuestionPayload { questions: vec![] },
        })
    }

    #[test]
    fn merge_prefers_needs_user_input_over_ok() {
        let merged =
            merge_stream_results_by_id(vec![("t1".into(), ok_out()), ("t1".into(), needs_input())]);
        assert!(matches!(
            merged.get("t1"),
            Some(Err(ToolError::NeedsUserInput { .. }))
        ));
    }

    #[test]
    fn merge_keeps_needs_user_input_when_later_ok() {
        let merged =
            merge_stream_results_by_id(vec![("t1".into(), needs_input()), ("t1".into(), ok_out())]);
        assert!(matches!(
            merged.get("t1"),
            Some(Err(ToolError::NeedsUserInput { .. }))
        ));
    }

    #[test]
    fn ordered_results_follows_tool_call_order() {
        let mut by_id = HashMap::new();
        by_id.insert("b".into(), ok_out());
        by_id.insert("a".into(), ok_out());
        let order = ordered_tool_result_ids(&["a".into(), "b".into()], &by_id);
        assert_eq!(order, vec!["a", "b"]);
    }
}
