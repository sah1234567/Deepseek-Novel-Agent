#[derive(Debug, thiserror::Error)]
pub enum CompactionError {
    #[error("Tokenization error: {0}")]
    Tokenization(String),
    #[error("Context still too large after compaction: {tokens} tokens > window {window}")]
    ContextTooLarge { tokens: usize, window: usize },
}
