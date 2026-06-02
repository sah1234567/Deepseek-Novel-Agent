//! Work-directory listing helpers (no Tauri `AppHandle`).

use novel_tools::PermissionMode;
use std::path::Path;

/// Parse UI / IPC permission mode strings.
pub(crate) fn parse_permission_mode(mode: &str) -> Result<PermissionMode, String> {
    match mode {
        "normal" => Ok(PermissionMode::Normal),
        "plan" => Ok(PermissionMode::Plan),
        "auto" => Ok(PermissionMode::Auto),
        "unattended" => Ok(PermissionMode::Unattended),
        other => Err(format!("invalid permission mode: {other}")),
    }
}

/// Sorted work directory names under `works_root` (non-hidden directories only).
pub(crate) fn list_work_dirs(works_root: &Path) -> Vec<String> {
    if !works_root.is_dir() {
        return Vec::new();
    }
    let Ok(entries) = std::fs::read_dir(works_root) else {
        return Vec::new();
    };
    let mut names = Vec::new();
    for entry in entries.flatten() {
        let path = entry.path();
        if !path.is_dir() {
            continue;
        }
        let Some(name) = path
            .file_name()
            .and_then(|n| n.to_str())
            .filter(|n| !n.is_empty() && !n.starts_with('.'))
        else {
            continue;
        };
        names.push(name.to_string());
    }
    names.sort();
    names
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::TempDir;

    #[test]
    fn empty_when_root_missing() {
        assert!(list_work_dirs(Path::new("/nonexistent/works/root")).is_empty());
    }

    #[test]
    fn skips_hidden_files_and_non_dirs() {
        let tmp = TempDir::new().unwrap();
        let root = tmp.path();
        fs::create_dir(root.join("beta")).unwrap();
        fs::create_dir(root.join("alpha")).unwrap();
        fs::create_dir(root.join(".hidden")).unwrap();
        fs::write(root.join("not-a-dir.txt"), "x").unwrap();
        assert_eq!(
            list_work_dirs(root),
            vec!["alpha".to_string(), "beta".to_string()]
        );
    }

    #[test]
    fn parse_permission_mode_accepts_known_values() {
        assert!(matches!(
            parse_permission_mode("auto").unwrap(),
            PermissionMode::Auto
        ));
        assert!(matches!(
            parse_permission_mode("unattended").unwrap(),
            PermissionMode::Unattended
        ));
        assert!(parse_permission_mode("bogus").is_err());
    }

    #[test]
    fn empty_when_root_is_file() {
        let tmp = TempDir::new().unwrap();
        let f = tmp.path().join("file");
        fs::write(&f, "x").unwrap();
        assert!(list_work_dirs(&f).is_empty());
    }
}
