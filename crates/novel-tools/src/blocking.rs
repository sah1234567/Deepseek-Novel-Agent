//! Run synchronous I/O on Tokio's blocking thread pool (keeps async workers free).

use crate::ToolError;
use std::path::Path;

pub async fn run_blocking<F, T>(f: F) -> Result<T, ToolError>
where
    F: FnOnce() -> Result<T, ToolError> + Send + 'static,
    T: Send + 'static,
{
    tokio::task::spawn_blocking(f)
        .await
        .map_err(|e| ToolError::Internal(e.to_string()))?
}

pub async fn read_to_string(path: impl AsRef<Path> + Send + 'static) -> Result<String, ToolError> {
    run_blocking(move || std::fs::read_to_string(path.as_ref()).map_err(ToolError::Io)).await
}

pub async fn write(
    path: impl AsRef<Path> + Send + 'static,
    content: String,
) -> Result<(), ToolError> {
    run_blocking(move || std::fs::write(path.as_ref(), content).map_err(ToolError::Io)).await
}

pub async fn create_dir_all(path: impl AsRef<Path> + Send + 'static) -> Result<(), ToolError> {
    run_blocking(move || std::fs::create_dir_all(path.as_ref()).map_err(ToolError::Io)).await
}
