#![deny(clippy::unwrap_used)]
#![cfg_attr(test, allow(clippy::unwrap_used))]

mod audit_status;
mod causality;
mod character;
mod derive;
mod error;
mod evolution_log;
mod index;
mod parser;
mod project_tree;
mod scaffold;
mod scaffold_templates;
mod text_util;

pub use audit_status::{
    ensure_audit_status, format_progress_hint, list_pending, mark_audited, parse_chapter_numbers,
    query_chapter, query_summary, AuditChapterRow, AuditKind, AuditStatusSummary, PendingFilter,
    AUDIT_STATUS_PATH,
};
pub use causality::{parse_causality_markdown, CausalityGraph, CausalityNode};
pub use character::{CharacterCategory, CharacterFrontmatter, CharacterStatus};
pub use derive::{
    derive_character_snapshot, derive_foreshadow_categories, derive_relation_cross_index,
};
pub use error::KnowledgeError;
pub use evolution_log::{append_evolution_log, find_table_last_row, TableRow};
pub use index::{ensure_index, rebuild_index};
pub use parser::parse_frontmatter;
pub use project_tree::{list_project_files, read_project_file, ProjectFileEntry};
pub use scaffold::init_project_scaffold;
pub use text_util::{truncate_bytes_utf8, truncate_chars, truncate_with_suffix, utf8_byte_prefix};

use std::path::{Path, PathBuf};

pub struct KnowledgeStore {
    pub root: PathBuf,
}

impl KnowledgeStore {
    pub fn new(root: impl Into<PathBuf>) -> Self {
        Self { root: root.into() }
    }

    pub fn read_file(&self, rel: impl AsRef<Path>) -> Result<String, KnowledgeError> {
        let path = self.root.join(rel);
        let bytes = std::fs::read(&path).map_err(|e| {
            if e.kind() == std::io::ErrorKind::NotFound {
                KnowledgeError::FileNotFound(path.display().to_string())
            } else {
                KnowledgeError::Io(e)
            }
        })?;
        String::from_utf8(bytes).map_err(|_| KnowledgeError::EncodingError {
            path: path.display().to_string(),
        })
    }

    pub fn write_file(&self, rel: impl AsRef<Path>, content: &str) -> Result<(), KnowledgeError> {
        let path = self.root.join(rel);
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).map_err(KnowledgeError::Io)?;
        }
        std::fs::write(path, content).map_err(KnowledgeError::Io)
    }

    pub fn character_path(&self, name: &str) -> PathBuf {
        self.root
            .join("knowledge/characters")
            .join(format!("{name}.md"))
    }
}
