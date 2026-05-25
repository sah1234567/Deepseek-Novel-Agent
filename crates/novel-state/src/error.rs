#[derive(Debug, thiserror::Error)]
pub enum StateError {
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),
    #[error("Database error: {0}")]
    Database(#[from] rusqlite::Error),
    #[error("Pool error: {0}")]
    Pool(#[from] r2d2::Error),
    #[error("JSON error: {0}")]
    Json(#[from] serde_json::Error),
    #[error("Session not found: {0}")]
    SessionNotFound(String),
    #[error("Checkpoint not found: {0}")]
    CheckpointNotFound(String),
    #[error("Fork run not found: {0}")]
    ForkRunNotFound(String),
    #[error("Database corrupted")]
    DatabaseCorrupted,
}
