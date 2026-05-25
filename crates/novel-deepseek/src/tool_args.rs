use serde_json::Value;

#[derive(Debug, thiserror::Error, PartialEq)]
pub enum ToolParseError {
    #[error("Empty tool arguments")]
    EmptyArguments,
    #[error("Invalid JSON: {0}")]
    InvalidJson(String),
}

/// Parse model tool-call arguments string → JSON Value.
/// - trim whitespace
/// - empty string → `{}` (zero-arg tools)
/// - strict JSON, no repair heuristic
pub fn parse_tool_arguments(raw: &str) -> Result<Value, ToolParseError> {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return Ok(Value::Object(Default::default()));
    }
    serde_json::from_str(trimmed).map_err(|e| ToolParseError::InvalidJson(e.to_string()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_valid_json() {
        let v = parse_tool_arguments(r#"{"path":"a.md"}"#).unwrap();
        assert_eq!(v["path"], "a.md");
    }

    #[test]
    fn empty_becomes_empty_object() {
        assert_eq!(parse_tool_arguments("").unwrap(), Value::Object(Default::default()));
        assert_eq!(parse_tool_arguments("  ").unwrap(), Value::Object(Default::default()));
    }

    #[test]
    fn invalid_json_errors() {
        assert!(matches!(
            parse_tool_arguments("{bad"),
            Err(ToolParseError::InvalidJson(_))
        ));
    }
}
