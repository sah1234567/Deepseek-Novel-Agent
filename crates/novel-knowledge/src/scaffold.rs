use crate::scaffold_templates::{load_scaffold_templates, SCAFFOLD_DIRS};
use crate::{ensure_index, KnowledgeError, KnowledgeStore};
use std::path::Path;

/// Create required project directories and starter template files from `{agent_root}/templates/`.
pub fn init_project_scaffold(
    work_root: impl AsRef<Path>,
    templates_dir: &Path,
) -> Result<(), KnowledgeError> {
    let work_root = work_root.as_ref();
    let store = KnowledgeStore::new(work_root);

    for dir in SCAFFOLD_DIRS {
        std::fs::create_dir_all(work_root.join(dir)).map_err(KnowledgeError::Io)?;
    }

    for (rel, content) in load_scaffold_templates(templates_dir)? {
        if !work_root.join(&rel).exists() {
            store.write_file(&rel, &content)?;
        }
    }

    let _ = ensure_index(&store)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::scaffold_templates::repo_templates_dir;
    use tempfile::TempDir;

    #[test]
    fn scaffold_creates_dirs_and_templates_from_repo_templates() {
        let tmp = TempDir::new().unwrap();
        init_project_scaffold(tmp.path(), &repo_templates_dir()).unwrap();
        assert!(tmp.path().join("knowledge/INDEX.md").exists());
        assert!(tmp
            .path()
            .join("knowledge/characters/_template.md")
            .exists());
        assert!(tmp.path().join("AGENTS.md").exists());
        assert!(tmp.path().join("memory/MEMORY.md").exists());
        assert!(tmp.path().join("memory/genre.md").exists());
        assert!(tmp.path().join("memory/decisions.md").exists());
        assert!(tmp.path().join("knowledge/plot/大纲.md").exists());
        assert!(!tmp.path().join("plot").exists());
        assert!(!tmp.path().join("shared-systems").exists());
        assert!(!tmp.path().join("skills").exists());
    }

    #[test]
    fn scaffold_loads_from_custom_templates_dir() {
        let agent = TempDir::new().unwrap();
        let templates = agent.path().join("templates");
        std::fs::create_dir_all(templates.join("memory")).unwrap();
        std::fs::write(templates.join("AGENTS.md"), "# custom agents\n").unwrap();
        std::fs::write(templates.join("memory/MEMORY.md"), "# custom memory\n").unwrap();

        let work = TempDir::new().unwrap();
        init_project_scaffold(work.path(), &templates).unwrap();
        let agents = std::fs::read_to_string(work.path().join("AGENTS.md")).unwrap();
        assert!(agents.contains("custom agents"));
    }

    #[test]
    fn scaffold_errors_when_templates_dir_missing() {
        let tmp = TempDir::new().unwrap();
        let err =
            init_project_scaffold(tmp.path(), Path::new("/nonexistent/templates")).unwrap_err();
        assert!(matches!(err, KnowledgeError::TemplatesNotFound(_)));
    }
}
