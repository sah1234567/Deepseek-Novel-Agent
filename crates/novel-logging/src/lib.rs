use std::path::Path;
use tracing_subscriber::{fmt, layer::SubscriberExt, util::SubscriberInitExt, EnvFilter};

pub mod audit;
pub use audit::{AuditLogger, LogEvent};

static INIT: std::sync::Once = std::sync::Once::new();

const DEFAULT_FILTER: &str =
    "novel_agent=info,novel_core=info,novel_deepseek=info,novel_tools=info,novel_server=info";

fn wants_file_log(project_root: Option<&Path>) -> Option<std::path::PathBuf> {
    let root = project_root?;
    let force = std::env::var("NOVEL_DEBUG_LOG")
        .ok()
        .is_some_and(|v| v == "1" || v.eq_ignore_ascii_case("true"));
    if force || cfg!(debug_assertions) {
        Some(root.join(".novel/logs/debug.log"))
    } else {
        None
    }
}

/// Initialize tracing: human-readable stderr, optional JSON file at `{project_root}/.novel/logs/debug.log`.
pub fn init_logging(project_root: Option<&Path>) {
    INIT.call_once(|| {
        let filter = EnvFilter::try_from_default_env()
            .unwrap_or_else(|_| EnvFilter::new(DEFAULT_FILTER));
        let stderr_layer = fmt::layer().with_target(true).with_thread_ids(false);

        if let Some(log_path) = wants_file_log(project_root) {
            if let Some(parent) = log_path.parent() {
                let _ = std::fs::create_dir_all(parent);
            }
            if let Ok(f) = std::fs::OpenOptions::new()
                .create(true)
                .append(true)
                .open(&log_path)
            {
                let json_layer = fmt::layer().json().with_writer(std::sync::Mutex::new(f));
                let _ = tracing_subscriber::registry()
                    .with(filter)
                    .with(stderr_layer)
                    .with(json_layer)
                    .try_init();
                return;
            }
        }

        let _ = tracing_subscriber::registry()
            .with(filter)
            .with(stderr_layer)
            .try_init();
    });
}

/// Initialize tracing to stderr only (no project root).
pub fn init() {
    init_logging(None);
}

/// Backward-compatible alias: enables file log when `NOVEL_DEBUG_LOG=1` or debug build.
pub fn init_with_json_log(project_root: &Path) {
    init_logging(Some(project_root));
}
