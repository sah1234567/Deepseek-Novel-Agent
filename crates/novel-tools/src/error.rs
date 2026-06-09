use crate::AskUserQuestionPayload;
use serde_json::Value;

#[derive(Debug, thiserror::Error)]
pub enum ToolError {
    #[error("Validation error: {0}")]
    Validation(#[from] ValidationError),
    #[error("Permission denied: {0}")]
    PermissionDenied(String),
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),
    #[error("Unknown tool: {0}")]
    UnknownTool(String),
    #[error("Execution error: {0}")]
    Execution(String),
    #[error("Knowledge error: {0}")]
    Knowledge(#[from] novel_knowledge::KnowledgeError),
    #[error("Internal error: {0}")]
    Internal(String),
    #[error("Needs user input")]
    NeedsUserInput { payload: AskUserQuestionPayload },
}

#[derive(Debug, thiserror::Error, PartialEq)]
pub enum ValidationError {
    #[error("Missing required field: {0}")]
    MissingField(String),
    #[error("Invalid field: {0}")]
    InvalidField(String),
}

pub(crate) fn require_str(input: &Value, key: &str) -> Result<String, ValidationError> {
    input
        .get(key)
        .and_then(|v| v.as_str())
        .map(|s| s.to_string())
        .ok_or_else(|| ValidationError::MissingField(key.into()))
}
