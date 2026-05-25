use crate::ConfigError;
use std::path::{Path, PathBuf};

const WORKS_DIR: &str = "works";
const SKILLS_DIR: &str = "skills";
const TEMPLATES_DIR: &str = "templates";
const AGENT_DOT_DIR: &str = ".novel-agent";
const API_CONFIG_FILE: &str = "api_config.json";

/// Walk upward from `start` until a directory containing `skills/` is found.
pub fn resolve_agent_root() -> PathBuf {
    let mut candidates: Vec<PathBuf> = Vec::new();
    if let Ok(exe) = std::env::current_exe() {
        if let Some(parent) = exe.parent() {
            candidates.push(parent.to_path_buf());
        }
    }
    if let Ok(cwd) = std::env::current_dir() {
        candidates.push(cwd);
    }
    for start in candidates {
        let mut dir = start;
        loop {
            if dir.join(SKILLS_DIR).is_dir() {
                return dir;
            }
            if !dir.pop() {
                break;
            }
        }
    }
    std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."))
}

pub fn works_dir(agent_root: impl AsRef<Path>) -> PathBuf {
    agent_root.as_ref().join(WORKS_DIR)
}

pub fn skills_dir(agent_root: impl AsRef<Path>) -> PathBuf {
    agent_root.as_ref().join(SKILLS_DIR)
}

pub fn templates_dir(agent_root: impl AsRef<Path>) -> PathBuf {
    agent_root.as_ref().join(TEMPLATES_DIR)
}

pub fn global_config_dir(agent_root: impl AsRef<Path>) -> PathBuf {
    agent_root.as_ref().join(AGENT_DOT_DIR)
}

pub fn global_api_config_path(agent_root: impl AsRef<Path>) -> PathBuf {
    global_config_dir(agent_root).join(API_CONFIG_FILE)
}

pub fn work_path(agent_root: impl AsRef<Path>, name: &str) -> Result<PathBuf, ConfigError> {
    let name = validate_work_name(name)?;
    Ok(works_dir(agent_root).join(name))
}

/// Sanitize and validate a work folder name (no path separators or `..`).
pub fn validate_work_name(name: &str) -> Result<String, ConfigError> {
    let trimmed = name.trim();
    if trimmed.is_empty() {
        return Err(ConfigError::InvalidValue("work name must not be empty".into()));
    }
    if trimmed.contains('/') || trimmed.contains('\\') || trimmed.contains("..") {
        return Err(ConfigError::InvalidValue(
            "work name must not contain path separators".into(),
        ));
    }
    Ok(trimmed.to_string())
}

/// Ensure `path` is a canonical child of `works_dir`.
pub fn ensure_work_under_works(works_dir: &Path, path: &Path) -> Result<(), ConfigError> {
    let works_canon = works_dir
        .canonicalize()
        .unwrap_or_else(|_| works_dir.to_path_buf());
    let work_canon = if path.exists() {
        path.canonicalize().map_err(ConfigError::Io)?
    } else if let Some(parent) = path.parent() {
        if parent.exists() {
            parent
                .canonicalize()
                .map_err(ConfigError::Io)?
                .join(
                    path.file_name()
                        .ok_or_else(|| ConfigError::InvalidValue("invalid work path".into()))?,
                )
        } else {
            path.to_path_buf()
        }
    } else {
        path.to_path_buf()
    };
    if !work_canon.starts_with(&works_canon) {
        return Err(ConfigError::InvalidValue(format!(
            "work path {} is outside {}",
            path.display(),
            works_dir.display()
        )));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn validate_work_name_rejects_separators() {
        assert!(validate_work_name("my/novel").is_err());
        assert!(validate_work_name("..").is_err());
        assert_eq!(validate_work_name("  xianxia  ").unwrap(), "xianxia");
    }

    #[test]
    fn work_path_under_works() {
        let tmp = TempDir::new().unwrap();
        let p = work_path(tmp.path(), "demo").unwrap();
        assert!(p.ends_with("works/demo"));
    }
}
