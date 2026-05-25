//! Work scaffold templates loaded from `{agent_root}/templates/` at runtime.

use crate::KnowledgeError;
use std::path::Path;

/// Directories created before writing template files.
pub const SCAFFOLD_DIRS: &[&str] = &[
    "plan",
    "knowledge/characters",
    "knowledge/plot",
    "knowledge/plot/细纲",
    "knowledge/shared-systems",
    "knowledge/worlds",
    "chapters",
    "memory",
];

/// Load template files from `{agent_root}/templates/`. Errors if missing or empty.
pub fn load_scaffold_templates(templates_dir: &Path) -> Result<Vec<(String, String)>, KnowledgeError> {
    if !templates_dir.is_dir() {
        return Err(KnowledgeError::TemplatesNotFound(
            templates_dir.display().to_string(),
        ));
    }
    let mut files = Vec::new();
    for entry in walkdir::WalkDir::new(templates_dir)
        .into_iter()
        .filter_map(|e| e.ok())
    {
        if !entry.file_type().is_file() {
            continue;
        }
        let rel = entry
            .path()
            .strip_prefix(templates_dir)
            .map_err(|_| KnowledgeError::InvalidPath(entry.path().display().to_string()))?;
        let rel_str = rel.to_string_lossy().replace('\\', "/");
        let content = std::fs::read_to_string(entry.path()).map_err(KnowledgeError::Io)?;
        files.push((rel_str, content));
    }
    if files.is_empty() {
        return Err(KnowledgeError::TemplatesNotFound(
            templates_dir.display().to_string(),
        ));
    }
    files.sort_by(|a, b| a.0.cmp(&b.0));
    Ok(files)
}

#[cfg(test)]
pub fn repo_templates_dir() -> std::path::PathBuf {
    std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../templates")
}
