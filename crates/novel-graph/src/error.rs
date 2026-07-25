use thiserror::Error;

#[derive(Debug, Error)]
pub enum GraphError {
    #[error("plan validation failed: {0}")]
    Validation(String),
    #[error("graph state error: {0}")]
    State(String),
    #[error("node not found: {0}")]
    NodeNotFound(String),
    #[error("loop not found: {0}")]
    LoopNotFound(String),
    #[error("illegal transition: {0}")]
    IllegalTransition(String),
    #[error("gate denied: {0}")]
    GateDenied(String),
    #[error("io error: {0}")]
    Io(#[from] std::io::Error),
    #[error("json error: {0}")]
    Json(#[from] serde_json::Error),
}

pub type GraphResult<T> = Result<T, GraphError>;
