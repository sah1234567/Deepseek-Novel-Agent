//! Audit findings extraction — scans `knowledge/meta/audits/` for findings marked
//! `[可泛化]` and groups them per chapter.
//!
//! Closure data source for the findings→rules loop:
//! every 10 chapters a GeneralPurpose subagent reads the recent findings, promotes
//! patterns repeated ≥2 times into `memory/rejected_paths/` or `memory/style/`,
//! and marks them 「已沉淀」 in `knowledge/meta/findings/`.

use crate::KnowledgeStore;

/// One extracted, generalizable finding.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Finding {
    pub chapter: u32,
    pub text: String,
}

/// Extract the chapter number from an audit report filename
/// (`chapter-005-pa.md` / `chapter-012-ka.md` / `chapter-003-cca.md`).
fn chapter_from_audit_filename(name: &str) -> Option<u32> {
    let rest = name.strip_prefix("chapter-")?;
    let digits: String = rest.chars().take_while(|c| c.is_ascii_digit()).collect();
    digits.parse().ok()
}

/// Scan `knowledge/meta/audits/` and return every line containing `[可泛化]`,
/// sorted by filename (chapter order). Missing directory → empty vec.
pub fn extract_generalizable_findings(store: &KnowledgeStore) -> Vec<Finding> {
    let audits_dir = store.root.join("knowledge/meta/audits");
    let Ok(entries) = std::fs::read_dir(&audits_dir) else {
        return Vec::new();
    };
    let mut files: Vec<_> = entries.flatten().collect();
    files.sort_by_key(|e| e.file_name());
    let mut out = Vec::new();
    for entry in files {
        let name = entry.file_name().to_string_lossy().to_string();
        let Some(chapter) = chapter_from_audit_filename(&name) else {
            continue;
        };
        let Ok(content) = std::fs::read_to_string(entry.path()) else {
            continue;
        };
        for line in content.lines() {
            if line.contains("[可泛化]") {
                out.push(Finding {
                    chapter,
                    text: line.trim().to_string(),
                });
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    fn write_audit(root: &std::path::Path, name: &str, content: &str) {
        std::fs::create_dir_all(root.join("knowledge/meta/audits")).unwrap();
        std::fs::write(root.join("knowledge/meta/audits").join(name), content).unwrap();
    }

    #[test]
    fn extracts_generalizable_lines_in_chapter_order() {
        let tmp = TempDir::new().unwrap();
        write_audit(
            tmp.path(),
            "chapter-005-pa.md",
            "伏笔密度: 本章新埋 4 条 [可泛化] 伏笔堆积\n因果闭合: 无断头边\n",
        );
        write_audit(
            tmp.path(),
            "chapter-003-cca.md",
            "对话标签重复: 「说道」5 次 [可泛化]\n节奏正常\n",
        );
        let store = KnowledgeStore::new(tmp.path());
        let f = extract_generalizable_findings(&store);
        assert_eq!(f.len(), 2);
        assert_eq!(f[0].chapter, 3); // filename order, not content order
        assert!(f[0].text.contains("对话标签"));
        assert_eq!(f[1].chapter, 5);
        assert!(f[1].text.contains("伏笔堆积"));
    }

    #[test]
    fn ignores_non_audit_files_and_missing_dir() {
        let tmp = TempDir::new().unwrap();
        let store = KnowledgeStore::new(tmp.path());
        assert!(extract_generalizable_findings(&store).is_empty());
        write_audit(tmp.path(), "notes.md", "普通笔记 [可泛化]");
        assert!(extract_generalizable_findings(&store).is_empty());
    }
}
