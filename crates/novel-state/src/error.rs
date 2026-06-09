#[derive(Debug, thiserror::Error)]
pub enum StateError {
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),
    #[error("Database error: {0}")]
    Database(String),
    #[error("Pool error: {0}")]
    Pool(String),
    #[error("JSON error: {0}")]
    Json(#[from] serde_json::Error),
    #[error("Session not found: {0}")]
    SessionNotFound(String),
    #[error("Fork run not found: {0}")]
    ForkRunNotFound(String),
    #[error("Database corrupted")]
    DatabaseCorrupted,
    #[error("{0}")]
    Validation(String),
}

impl From<rusqlite::Error> for StateError {
    fn from(e: rusqlite::Error) -> Self {
        StateError::Database(e.to_string())
    }
}

impl From<r2d2::Error> for StateError {
    fn from(e: r2d2::Error) -> Self {
        StateError::Pool(e.to_string())
    }
}
