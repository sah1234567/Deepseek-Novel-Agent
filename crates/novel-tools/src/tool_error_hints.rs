//! Actionable error hints appended to tool failures returned to the LLM.

use crate::ToolError;
use serde_json::Value;

pub fn enhance_tool_error_for_llm(
    tool_name: &str,
    err: &ToolError,
    tool_input: Option<&Value>,
) -> String {
    let base = format!("Error: {err}");
    let hint = match err {
        ToolError::Execution(msg) => hint_for_execution(tool_name, msg.as_str(), tool_input),
        _ => None,
    };
    match hint {
        Some(h) => format!("{base}\n\nNext steps: {h}"),
        None => base,
    }
}

fn hint_for_execution(
    tool_name: &str,
    msg: &str,
    _tool_input: Option<&Value>,
) -> Option<&'static str> {
    if msg.contains("Read economy:") {
        return Some("Use Grep to locate line numbers, then Read with offset+limit or Tail for file-end segments.");
    }
    if tool_name == "Edit" || tool_name == "Write" {
        if msg.contains("before editing") || msg.contains("before overwriting") {
            return Some("Read or Tail the target file (or the lines containing your edit) before Write/Edit.");
        }
        if msg.contains("modified since last read") {
            return Some("Read or Tail the file again, then retry Edit.");
        }
        if msg.contains("only read a portion") || msg.contains("not in the read slice") {
            return Some(
                "Re-Read with offset/limit covering old_string (Grep first for line numbers).",
            );
        }
        if msg.contains("line number prefix") {
            return Some("Remove \"{n}\\t\" prefixes from old_string; match raw file text only.");
        }
        if msg.contains("not found in") {
            return Some("Grep to locate text, Read ±3 lines around the match, copy exact whitespace into old_string.");
        }
        if msg.contains("matches of old_string") || msg.contains("replace_all") {
            return Some("Set replace_all:true OR expand old_string with 2–4 surrounding lines for uniqueness.");
        }
        if msg.contains("identical") {
            return Some("Change new_string so it differs from old_string.");
        }
    }
    if tool_name == "Tail" && msg.contains("exceeds max") {
        return Some("Reduce lines parameter or use Read offset/limit for middle segments.");
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use rstest::rstest;

    #[test]
    fn edit_not_read_hint() {
        let err =
            ToolError::Execution("Read foo.md before editing (read-before-write policy)".into());
        let s = enhance_tool_error_for_llm("Edit", &err, None);
        assert!(s.contains("Next steps:"));
        assert!(s.contains("Read or Tail"));
    }

    #[rstest]
    #[case("Read", "Read economy: knowledge/foo.md", Some("Grep"))]
    #[case("Edit", "Read foo.md before editing", Some("Read or Tail"))]
    #[case("Write", "Read bar.md before overwriting", Some("Read or Tail"))]
    #[case("Edit", "modified since last read", Some("Read or Tail"))]
    #[case("Edit", "only read a portion of this file", Some("offset/limit"))]
    #[case("Edit", "not in the read slice", Some("offset/limit"))]
    #[case("Edit", "line number prefix in old_string", Some("prefixes"))]
    #[case("Edit", "not found in file", Some("Grep"))]
    #[case("Edit", "3 matches of old_string", Some("replace_all"))]
    #[case("Edit", "old_string and new_string are identical", Some("differs"))]
    #[case("Tail", "requested lines exceeds max", Some("Reduce lines"))]
    #[case("Grep", "pattern not found", None)]
    #[case("Read", "file not found", None)]
    fn hint_for_execution_cases(
        #[case] tool_name: &str,
        #[case] msg: &str,
        #[case] expect_substr: Option<&str>,
    ) {
        let hint = hint_for_execution(tool_name, msg, None);
        match expect_substr {
            Some(sub) => {
                let h = hint.expect("expected hint");
                assert!(h.contains(sub), "hint {:?} should contain {:?}", h, sub);
            }
            None => assert!(hint.is_none()),
        }
    }

    #[rstest]
    #[case("Edit", "Read foo before editing", true)]
    #[case("Read", "Read economy: cap", true)]
    #[case("Grep", "no match", false)]
    fn enhance_tool_error_appends_next_steps(
        #[case] tool_name: &str,
        #[case] msg: &str,
        #[case] with_steps: bool,
    ) {
        let err = ToolError::Execution(msg.into());
        let s = enhance_tool_error_for_llm(tool_name, &err, None);
        assert_eq!(s.contains("Next steps:"), with_steps);
    }
}
