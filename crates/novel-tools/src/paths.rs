//! Cross-platform path helpers: JSON extraction, normalization, project-root resolution.

use crate::ValidationError;
use serde_json::Value;
use std::path::{Path, PathBuf};

/// Normalize a relative path for comparison and classification (`\` → `/`, trim `./`).
pub fn normalize_rel_path(rel: &str) -> String {
    let norm = rel.replace('\\', "/");
    norm.trim_start_matches("./").to_string()
}

/// Resolve `rel` under `project_root`. Absolute paths pass through unchanged.
pub fn resolve_under_project(project_root: &Path, rel: &str) -> PathBuf {
    let normalized = rel.replace('/', std::path::MAIN_SEPARATOR_STR);
    let p = PathBuf::from(normalized);
    if p.is_absolute() {
        p
    } else {
        project_root.join(p)
    }
}

/// Required `file_path` for Read / Write / Edit / Tail.
pub fn extract_file_path(input: &Value) -> Result<String, ValidationError> {
    crate::require_str(input, "file_path")
}

/// Optional `file_path` for side-chain logic (permissions, hooks, tracking).
pub fn optional_file_path(input: &Value) -> Option<String> {
    input
        .get("file_path")
        .and_then(|v| v.as_str())
        .map(|s| s.to_string())
        .filter(|s| !s.is_empty())
}

/// Optional `search_root` for Grep / Glob (defaults to project root when absent).
pub fn optional_search_root(input: &Value) -> Option<String> {
    input
        .get("search_root")
        .and_then(|v| v.as_str())
        .map(|s| s.to_string())
        .filter(|s| !s.is_empty())
}

/// Progress tracking: ensure `chapters/` prefix on chapter paths.
pub fn normalize_chapter_progress_path(rel: &str) -> String {
    let norm = normalize_rel_path(rel);
    if norm.starts_with("chapters/") {
        norm
    } else {
        format!("chapters/{}", norm.trim_start_matches('/'))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn normalize_rel_path_forward_slashes_and_trims_dot_slash() {
        assert_eq!(
            normalize_rel_path(".\\knowledge\\foo.md"),
            "knowledge/foo.md"
        );
        assert_eq!(normalize_rel_path("./chapters/a.md"), "chapters/a.md");
    }

    #[test]
    fn resolve_under_project_joins_relative() {
        let root = PathBuf::from("/project");
        let p = resolve_under_project(&root, "chapters/chapter-001.md");
        assert!(p.to_string_lossy().contains("chapter-001.md"));
        assert!(p.starts_with(&root));
    }

    #[test]
    fn resolve_under_project_passes_absolute() {
        let root = PathBuf::from("/project");
        #[cfg(windows)]
        let abs = "C:\\skills\\x\\references\\y.md";
        #[cfg(not(windows))]
        let abs = "/skills/x/references/y.md";
        let p = resolve_under_project(&root, abs);
        assert!(p.is_absolute());
    }

    #[test]
    fn extract_file_path_requires_field() {
        assert!(extract_file_path(&json!({"file_path": "a.md"})).is_ok());
        assert!(extract_file_path(&json!({"path": "a.md"})).is_err());
    }

    #[test]
    fn optional_search_root_ignores_path_alias() {
        assert_eq!(
            optional_search_root(&json!({"search_root": "."})),
            Some(".".into())
        );
        assert_eq!(optional_search_root(&json!({"path": "."})), None);
    }

    #[test]
    fn normalize_chapter_progress_path_adds_prefix() {
        assert_eq!(
            normalize_chapter_progress_path("chapter-005.md"),
            "chapters/chapter-005.md"
        );
        assert_eq!(
            normalize_chapter_progress_path("chapters/chapter-005.md"),
            "chapters/chapter-005.md"
        );
    }
}
