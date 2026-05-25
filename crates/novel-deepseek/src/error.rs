#[derive(Debug, thiserror::Error)]
pub enum LlmError {
    #[error("API error: {0}")]
    Api(String),
    #[error("Rate limited: {0}")]
    RateLimited(String),
    #[error("Empty response from API")]
    EmptyResponse,
    #[error("Tool parse error: {0}")]
    ToolParse(String),
    #[error("Missing API key — set DEEPSEEK_API_KEY")]
    MissingApiKey,
    #[error("Request cancelled")]
    Cancelled,
    #[error("Context length exceeded: {body}")]
    ContextLengthExceeded { body: String },
}

pub fn is_context_length_exceeded(err: &LlmError) -> bool {
    matches!(err, LlmError::ContextLengthExceeded { .. })
}

pub fn is_output_truncated(stop_reason: Option<&str>) -> bool {
    stop_reason == Some("length")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detects_context_length_exceeded() {
        let err = LlmError::ContextLengthExceeded {
            body: "maximum context length is 131072".into(),
        };
        assert!(is_context_length_exceeded(&err));
        assert!(!is_context_length_exceeded(&LlmError::Api("other".into())));
    }

    #[test]
    fn detects_output_truncation() {
        assert!(is_output_truncated(Some("length")));
        assert!(!is_output_truncated(Some("stop")));
        assert!(!is_output_truncated(None));
    }
}
