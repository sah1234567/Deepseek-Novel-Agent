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

/// Write file content, using atomic rename for `memory/` paths.
pub async fn write_for_rel_path(
    rel_path: &str,
    path: impl AsRef<Path> + Send + 'static,
    content: String,
) -> Result<(), ToolError> {
    if novel_memory::is_memory_rel_path(rel_path) {
        write_atomic(path, content).await
    } else {
        write(path, content).await
    }
}

pub async fn create_dir_all(path: impl AsRef<Path> + Send + 'static) -> Result<(), ToolError> {
    run_blocking(move || std::fs::create_dir_all(path.as_ref()).map_err(ToolError::Io)).await
}

/// Atomic write: temp file + OS-level rename.
///
/// Used for all `memory/` writes so concurrent readers (e.g. `scan_memory_files`
/// during background prefetch/extraction) never see a half-written file.
///
/// The temp file is created in the same directory as the target to ensure
/// they are on the same filesystem (required for atomic `rename`).
pub async fn write_atomic(
    path: impl AsRef<Path> + Send + 'static,
    content: String,
) -> Result<(), ToolError> {
    run_blocking(move || {
        let target = path.as_ref();
        if let Some(parent) = target.parent() {
            std::fs::create_dir_all(parent).map_err(ToolError::Io)?;
        }
        // Temp file in same directory → same filesystem → rename is atomic
        let tmp = target.with_extension(format!(
            "md.tmp.{}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_nanos().to_string())
                .unwrap_or_else(|_| "0".into())
        ));
        std::fs::write(&tmp, &content).map_err(ToolError::Io)?;
        std::fs::rename(&tmp, target).map_err(ToolError::Io)?;
        Ok(())
    })
    .await
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::sync::{Arc, Barrier};
    use std::thread;
    use std::time::Duration;
    use tempfile::TempDir;

    #[tokio::test(flavor = "current_thread")]
    async fn write_for_rel_path_uses_atomic_for_memory() {
        let tmp = TempDir::new().unwrap();
        let target = tmp.path().join("memory").join("style").join("pacing.md");
        write_for_rel_path("memory/style/pacing.md", target.clone(), "body".into())
            .await
            .unwrap();
        assert_eq!(fs::read_to_string(target).unwrap(), "body");
        assert!(
            tmp.path()
                .join("memory/style")
                .read_dir()
                .unwrap()
                .all(|e| e.unwrap().path().extension().is_none_or(|x| x != "tmp")),
            "temp files should be cleaned up after rename"
        );
    }

    #[tokio::test(flavor = "current_thread")]
    async fn write_atomic_never_exposes_partial_content() {
        let tmp = TempDir::new().unwrap();
        let target = tmp.path().join("memory").join("note.md");
        fs::create_dir_all(target.parent().unwrap()).unwrap();

        let target_for_writer = target.clone();
        let barrier = Arc::new(Barrier::new(2));
        let reader_barrier = barrier.clone();

        let writer = thread::spawn(move || {
            let rt = tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
                .unwrap();
            rt.block_on(async {
                run_blocking(move || {
                    let tmp_file = target_for_writer.with_extension("md.tmp.test");
                    fs::write(&tmp_file, "partial").unwrap();
                    reader_barrier.wait();
                    thread::sleep(Duration::from_millis(20));
                    fs::write(&tmp_file, "complete").unwrap();
                    fs::rename(&tmp_file, &target_for_writer).unwrap();
                    Ok::<(), ToolError>(())
                })
                .await
                .unwrap();
            });
        });

        barrier.wait();
        // While writer holds temp file, target must not exist or contain partial bytes
        if target.exists() {
            let peek = fs::read_to_string(&target).unwrap_or_default();
            assert_ne!(peek, "partial");
        }
        writer.join().unwrap();
        assert_eq!(fs::read_to_string(&target).unwrap(), "complete");
    }
}
