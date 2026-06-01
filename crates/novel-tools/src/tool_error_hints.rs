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

    #[test]
    fn edit_not_read_hint() {
        let err =
            ToolError::Execution("Read foo.md before editing (read-before-write policy)".into());
        let s = enhance_tool_error_for_llm("Edit", &err, None);
        assert!(s.contains("Next steps:"));
        assert!(s.contains("Read or Tail"));
    }
}
