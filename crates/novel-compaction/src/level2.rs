//! Knowledge-base evolution log compression (Level 2).
//!
//! Unlike Level 1 (chapter-text replacement in tool_results) and Level 4 (turn
//! eviction), Level 2 modifies files on disk. Per the project architecture
//! principle, it is NOT auto-triggered by the compaction pipeline. The agent
//! invokes it explicitly via `KnowledgeDerive { compress_logs: true }`.

use crate::CompactionError;
use novel_knowledge::{compress_evolution_table, KnowledgeStore};
use std::path::Path;

/// Table headings that contain evolution logs eligible for compression.
const LOG_HEADINGS: &[&str] = &[
    "身份演变日志",
    "性格演变日志",
    "关系演变日志",
    "出场记录日志",
    "秘密演变日志",
    "伏笔演变",
    "实力演变",
    "身份发展",
];

/// Report after a Level 2 compression run.
#[derive(Debug, Clone)]
pub struct Level2Report {
    /// Number of files modified.
    pub files_compressed: usize,
    /// Total data rows merged into summary lines.
    pub total_rows_merged: usize,
}

/// Scan knowledge files for evolution-log tables and compress long ones.
///
/// For each `knowledge/characters/*.md` (ignoring `_`-prefixed templates),
/// and each known plot file (`knowledge/plot/伏笔追踪.md`), this function
/// finds any of the [`LOG_HEADINGS`] tables and runs
/// [`compress_evolution_table`] on rows exceeding `tail_rows`.
pub fn apply_level2_knowledge(
    project_root: &Path,
    tail_rows: usize,
) -> Result<Level2Report, CompactionError> {
    let store = KnowledgeStore::new(project_root);
    let mut report = Level2Report {
        files_compressed: 0,
        total_rows_merged: 0,
    };

    // Compress character cards
    let chars_dir = project_root.join("knowledge/characters");
    if chars_dir.is_dir() {
        if let Ok(entries) = std::fs::read_dir(&chars_dir) {
            for entry in entries.flatten() {
                let path = entry.path();
                let Some(name) = path.file_name().and_then(|n| n.to_str()) else {
                    continue;
                };
                if !name.ends_with(".md") || name.starts_with('_') {
                    continue;
                }
                let rel = format!("knowledge/characters/{name}");
                match compress_file(&store, &rel, tail_rows) {
                    Ok(merged) => {
                        if merged > 0 {
                            report.files_compressed += 1;
                            report.total_rows_merged += merged;
                        }
                    }
                    Err(_e) => {
                        // File might be malformed or missing; skip gracefully
                    }
                }
            }
        }
    }

    // Compress plot files with tables
    for plot_rel in &["knowledge/plot/伏笔追踪.md"] {
        match compress_file(&store, plot_rel, tail_rows) {
            Ok(merged) => {
                if merged > 0 {
                    report.files_compressed += 1;
                    report.total_rows_merged += merged;
                }
            }
            Err(_e) => {
                // File might not exist (optional knowledge file); skip gracefully
            }
        }
    }

    Ok(report)
}

fn compress_file(store: &KnowledgeStore, rel: &str, tail_rows: usize) -> Result<usize, CompactionError> {
    let content = store
        .read_file(rel)
        .map_err(|e| CompactionError::Tokenization(e.to_string()))?;
    let mut modified = content.clone();
    let mut file_merged = 0usize;

    for heading in LOG_HEADINGS {
        let compressed = compress_evolution_table(&modified, heading, tail_rows)
            .map_err(|e| CompactionError::Tokenization(e.to_string()))?;
        if compressed != modified {
            // Count how many data rows were merged
            let before_count = modified.matches("\n| ").count();
            let after_count = compressed.matches("\n| ").count();
            if before_count > after_count + 1 {
                // +1 for the summary row we insert
                file_merged = file_merged.saturating_add(before_count - after_count - 1);
            }
            modified = compressed;
        }
    }

    if file_merged > 0 {
        store
            .write_file(rel, &modified)
            .map_err(|e| CompactionError::Tokenization(e.to_string()))?;
    }
    Ok(file_merged)
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn level2_compresses_long_evolution_table() {
        let tmp = TempDir::new().unwrap();
        let chars_dir = tmp.path().join("knowledge/characters");
        std::fs::create_dir_all(&chars_dir).unwrap();

        // A character card with an evolution table longer than tail_rows
        let card = r#"---
name: 测试角色
status: alive
---
## 出场记录日志
| 章节 | 关键事件 | 伏笔关联 | 情绪弧线 |
|------|---------|---------|---------|
| Ch1  | 事件1   | F01     | 平静    |
| Ch2  | 事件2   | F02     | 兴奋    |
| Ch3  | 事件3   | -       | 悲伤    |
| Ch4  | 事件4   | F03     | 愤怒    |
| Ch5  | 事件5   | -       | 释然    |
| Ch6  | 事件6   | F04     | 紧张    |
| Ch7  | 事件7   | -       | 坚定    |
"#;
        std::fs::write(chars_dir.join("测试角色.md"), card).unwrap();

        let report = apply_level2_knowledge(tmp.path(), 3).unwrap();
        assert!(report.files_compressed > 0, "should have compressed at least 1 file");
        assert!(report.total_rows_merged > 0, "should have merged some rows");

        let compressed = std::fs::read_to_string(chars_dir.join("测试角色.md")).unwrap();
        assert!(compressed.contains("[压缩:"));
        // Should keep the last 3 data rows
        assert!(compressed.contains("Ch5"));
        assert!(compressed.contains("Ch6"));
        assert!(compressed.contains("Ch7"));
    }
}
