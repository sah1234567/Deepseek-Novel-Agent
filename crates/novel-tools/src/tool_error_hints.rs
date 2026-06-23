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
        return hint_for_read_economy(tool_name);
    }
    if tool_name == "Edit" || tool_name == "Write" {
        return hint_for_edit_write(msg);
    }
    if tool_name == "Tail" && msg.contains("exceeds max") {
        return Some("Reduce lines parameter or use Read offset/limit for middle segments.");
    }
    None
}

fn hint_for_read_economy(tool_name: &str) -> Option<&'static str> {
    if tool_name == "Grep" {
        Some("Narrow the pattern, add a glob filter by filename, or use head_limit/offset for pagination (e.g. head_limit=50 then offset=50 for next page).")
    } else {
        Some("Use Grep to locate line numbers, then Read with offset+limit or Tail for file-end segments.")
    }
}

fn hint_for_edit_write(msg: &str) -> Option<&'static str> {
    if msg.contains("before editing") || msg.contains("before overwriting") {
        return Some(
            "Read or Tail the target file (or the lines containing your edit) before Write/Edit.",
        );
    }
    if msg.contains("modified since last read") {
        return Some("Read or Tail the file again, then retry Edit.");
    }
    if msg.contains("was not applied") && msg.contains("not yet in the conversation") {
        return Some(
            "The Read/Tail in this turn will appear in tool_result before your next reply — retry Edit then (no re-Read if unchanged).",
        );
    }
    if msg.contains("not found on disk") || msg.contains("not a read-cache staleness") {
        return Some(
            "Grep a distinctive phrase (not the audit summary), Read ±3 lines around the match, copy exact bytes into old_string (strip line-number tabs). Do not retry identical Read params.",
        );
    }
    if msg.contains("only read a portion") || msg.contains("editable lines (seen in conversation)")
    {
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
        return Some(
            "Set replace_all:true OR expand old_string with 2–4 surrounding lines for uniqueness.",
        );
    }
    if msg.contains("identical") {
        return Some(
            "Target may already match (audit said keep/skip) — verify with Grep; if change needed, ensure new_string differs.",
        );
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
    #[case(
        "Edit",
        "old_string not found on disk (exact byte match required)",
        Some("Grep")
    )]
    #[case(
        "Edit",
        "was not applied: this line is only covered by a Read/Tail result not yet in the conversation",
        Some("retry Edit")
    )]
    #[case("Edit", "line number prefix in old_string", Some("prefixes"))]
    #[case("Edit", "not found in file", Some("Grep"))]
    #[case("Edit", "3 matches of old_string", Some("replace_all"))]
    #[case(
        "Edit",
        "old_string and new_string are identical",
        Some("audit said keep")
    )]
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
