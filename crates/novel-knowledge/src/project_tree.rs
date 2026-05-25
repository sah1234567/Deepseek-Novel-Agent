use crate::KnowledgeError;
use crate::KnowledgeStore;
use serde::{Deserialize, Serialize};
use std::path::{Component, Path};

const ROOT_DIRS: &[&str] = &["knowledge", "chapters", "plot", "memory", "worlds"];

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct ProjectFileEntry {
    pub path: String,
    pub is_dir: bool,
}

/// List project files under knowledge/chapters/plot/memory (relative POSIX paths).
pub fn list_project_files(root: impl AsRef<Path>) -> Result<Vec<ProjectFileEntry>, KnowledgeError> {
    let root = root.as_ref();
    let mut entries = Vec::new();

    for dir in ROOT_DIRS {
        let base = root.join(dir);
        if !base.is_dir() {
            continue;
        }
        walk_dir(root, &base, &mut entries)?;
    }

    entries.sort_by(|a, b| a.path.cmp(&b.path));
    Ok(entries)
}

fn walk_dir(
    project_root: &Path,
    dir: &Path,
    out: &mut Vec<ProjectFileEntry>,
) -> Result<(), KnowledgeError> {
    let mut stack: Vec<std::path::PathBuf> = vec![dir.to_path_buf()];
    while let Some(current) = stack.pop() {
        let rel = current
            .strip_prefix(project_root)
            .map_err(|_| KnowledgeError::InvalidPath(current.display().to_string()))?;
        let rel_str = rel.to_string_lossy().replace('\\', "/");
        if current != dir {
            out.push(ProjectFileEntry {
                path: rel_str.clone(),
                is_dir: current.is_dir(),
            });
        } else {
            out.push(ProjectFileEntry {
                path: rel_str,
                is_dir: true,
            });
        }

        if !current.is_dir() {
            continue;
        }

        let mut children: Vec<_> = std::fs::read_dir(&current)
            .map_err(KnowledgeError::Io)?
            .filter_map(|e| e.ok())
            .filter(|e| !should_skip(e.path().file_name()))
            .map(|e| e.path())
            .collect();
        children.sort_by(|a, b| a.file_name().cmp(&b.file_name()));
        for child in children {
            stack.push(child);
        }
    }
    Ok(())
}

fn should_skip(name: Option<&std::ffi::OsStr>) -> bool {
    let Some(name) = name else {
        return true;
    };
    let s = name.to_string_lossy();
    s.starts_with('.') || s == ".novel-agent"
}

/// Read a project file if the relative path is under allowed roots (no path traversal).
pub fn read_project_file(root: impl AsRef<Path>, rel_path: &str) -> Result<String, KnowledgeError> {
    let root = root.as_ref();
    let rel = Path::new(rel_path);
    if rel.is_absolute() || rel.components().any(|c| matches!(c, Component::ParentDir)) {
        return Err(KnowledgeError::InvalidPath(rel_path.to_string()));
    }

    let allowed = ROOT_DIRS
        .iter()
        .any(|d| rel_path == *d || rel_path.starts_with(&format!("{d}/")));
    if !allowed {
        return Err(KnowledgeError::InvalidPath(rel_path.to_string()));
    }

    let full = root.join(rel);
    let canonical_root = root.canonicalize().map_err(KnowledgeError::Io)?;
    let canonical_file = full.canonicalize().map_err(|e| {
        if e.kind() == std::io::ErrorKind::NotFound {
            KnowledgeError::FileNotFound(full.display().to_string())
        } else {
            KnowledgeError::Io(e)
        }
    })?;
    if !canonical_file.starts_with(&canonical_root) {
        return Err(KnowledgeError::InvalidPath(rel_path.to_string()));
    }
    if canonical_file.is_dir() {
        return Err(KnowledgeError::InvalidPath(rel_path.to_string()));
    }

    KnowledgeStore::new(root).read_file(rel_path)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::init_project_scaffold;
    use crate::scaffold_templates::repo_templates_dir;
    use tempfile::TempDir;

    #[test]
    fn list_project_files_after_scaffold() {
        let tmp = TempDir::new().unwrap();
        init_project_scaffold(tmp.path(), &repo_templates_dir()).unwrap();
        let files = list_project_files(tmp.path()).unwrap();
        let paths: Vec<_> = files.iter().map(|e| e.path.as_str()).collect();
        assert!(paths.iter().any(|p| p.contains("knowledge")));
        assert!(paths.iter().any(|p| p.ends_with("knowledge/INDEX.md")));
        assert!(paths.iter().any(|p| p.starts_with("chapters")));
    }

    #[test]
    fn read_project_file_rejects_traversal() {
        let tmp = TempDir::new().unwrap();
        init_project_scaffold(tmp.path(), &repo_templates_dir()).unwrap();
        assert!(read_project_file(tmp.path(), "../AGENTS.md").is_err());
        assert!(read_project_file(tmp.path(), "knowledge/../../AGENTS.md").is_err());
    }

    #[test]
    fn read_project_file_reads_index() {
        let tmp = TempDir::new().unwrap();
        init_project_scaffold(tmp.path(), &repo_templates_dir()).unwrap();
        let content = read_project_file(tmp.path(), "knowledge/INDEX.md").unwrap();
        assert!(!content.is_empty());
    }

    #[test]
    fn project_file_entry_serializes_camel_case() {
        let entry = ProjectFileEntry {
            path: "knowledge/characters".into(),
            is_dir: true,
        };
        let json = serde_json::to_string(&entry).unwrap();
        assert!(json.contains(r#""isDir":true"#));
        assert!(!json.contains("is_dir"));
    }

    #[test]
    fn list_nihao_work_chapters_if_present() {
        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../works/你好");
        if !root.is_dir() {
            return;
        }
        let files = list_project_files(&root).unwrap();
        assert!(files.iter().any(|e| e.path == "chapters"));
        assert!(files.iter().any(|e| e.path.starts_with("chapters/") && !e.is_dir));
    }

    #[test]
    fn read_nihao_chapter_if_present() {
        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../works/你好");
        if !root.is_dir() {
            return;
        }
        let content = read_project_file(&root, "chapters/chapter-001.md").unwrap();
        assert!(!content.is_empty());
    }
}
