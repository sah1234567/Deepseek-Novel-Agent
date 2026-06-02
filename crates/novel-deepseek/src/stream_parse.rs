//! SSE `data:` line parsing for [`crate::client::ChatClient::create_stream`].

/// Outcome of parsing one SSE line (trimmed by caller or internally).
#[derive(Debug)]
pub(crate) enum SseLine {
    /// Empty line, non-`data:` line, or `data:` with empty payload.
    Skip,
    /// Terminal `data: [DONE]`.
    Done,
    /// JSON object after `data: `.
    Data(serde_json::Value),
    /// `data:` present but payload is not valid JSON.
    InvalidJson(serde_json::Error),
}

/// Parse a single SSE line from the byte stream.
pub(crate) fn parse_sse_line(line: &str) -> SseLine {
    let line = line.trim();
    if line.is_empty() {
        return SseLine::Skip;
    }
    let Some(payload) = line.strip_prefix("data: ") else {
        return SseLine::Skip;
    };
    let payload = payload.trim();
    if payload.is_empty() {
        return SseLine::Skip;
    }
    if payload == "[DONE]" {
        return SseLine::Done;
    }
    match serde_json::from_str(payload) {
        Ok(v) => SseLine::Data(v),
        Err(e) => SseLine::InvalidJson(e),
    }
}

#[cfg(test)]
mod tests {
    use super::{parse_sse_line, SseLine};

    #[test]
    fn parses_json_after_data_prefix() {
        let SseLine::Data(v) = parse_sse_line(r#"data: {"choices":[]}"#) else {
            panic!("expected Data");
        };
        assert_eq!(v["choices"], serde_json::json!([]));
    }

    #[test]
    fn done_is_terminal() {
        assert!(matches!(parse_sse_line("data: [DONE]"), SseLine::Done));
    }

    #[test]
    fn skips_empty_and_non_data() {
        assert!(matches!(parse_sse_line(""), SseLine::Skip));
        assert!(matches!(parse_sse_line("   "), SseLine::Skip));
        assert!(matches!(parse_sse_line(r#"{"x":1}"#), SseLine::Skip));
    }

    #[test]
    fn skips_empty_payload() {
        assert!(matches!(parse_sse_line("data: "), SseLine::Skip));
        assert!(matches!(parse_sse_line("data:"), SseLine::Skip));
    }

    #[test]
    fn invalid_json_reports_error() {
        assert!(matches!(
            parse_sse_line("data: not-json"),
            SseLine::InvalidJson(_)
        ));
    }
}
