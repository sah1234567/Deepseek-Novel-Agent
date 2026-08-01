//! Parse author REGATE directives from free text.

/// Parse `REGATE: <node_id>` from free text. Returns the node id when a
/// REGATE line is present (REASON lines are tolerated but carry no state —
/// `reopen` has no reason storage).
pub fn parse_regate_directive(text: &str) -> Option<String> {
    for line in text.lines() {
        let trimmed = line.trim();
        if let Some(rest) = trimmed.strip_prefix("REGATE:") {
            let id = rest.trim();
            if !id.is_empty() {
                return Some(id.to_string());
            }
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_regate_only() {
        let got = parse_regate_directive("Please redo.\nREGATE: write-chapter\n").unwrap();
        assert_eq!(got, "write-chapter");
    }

    #[test]
    fn tolerates_reason_lines() {
        let text = "REGATE: fine-outline-batch\nREASON: timeline drift in Ch3\n";
        assert_eq!(
            parse_regate_directive(text).as_deref(),
            Some("fine-outline-batch")
        );
    }

    #[test]
    fn ignores_without_regate() {
        assert!(parse_regate_directive("REASON: orphan\n").is_none());
    }
}
