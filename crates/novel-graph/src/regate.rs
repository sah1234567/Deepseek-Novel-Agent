//! Parse author REGATE directives from free text.

/// Parse `REGATE: <node_id>` with optional following `REASON: ...` line.
/// Returns `(node_id, reason)` when a REGATE line is present.
pub fn parse_regate_directive(text: &str) -> Option<(String, Option<String>)> {
    let mut node_id = None;
    let mut reason = None;
    for line in text.lines() {
        let trimmed = line.trim();
        if let Some(rest) = trimmed.strip_prefix("REGATE:") {
            let id = rest.trim();
            if !id.is_empty() {
                node_id = Some(id.to_string());
            }
            continue;
        }
        if let Some(rest) = trimmed.strip_prefix("REASON:") {
            let r = rest.trim();
            if !r.is_empty() {
                reason = Some(r.to_string());
            }
        }
    }
    node_id.map(|id| (id, reason))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_regate_only() {
        let got = parse_regate_directive("Please redo.\nREGATE: write-chapter\n").unwrap();
        assert_eq!(got.0, "write-chapter");
        assert!(got.1.is_none());
    }

    #[test]
    fn parses_regate_with_reason() {
        let text = "REGATE: fine-outline-batch\nREASON: timeline drift in Ch3\n";
        let got = parse_regate_directive(text).unwrap();
        assert_eq!(got.0, "fine-outline-batch");
        assert_eq!(got.1.as_deref(), Some("timeline drift in Ch3"));
    }

    #[test]
    fn ignores_without_regate() {
        assert!(parse_regate_directive("REASON: orphan\n").is_none());
    }
}
