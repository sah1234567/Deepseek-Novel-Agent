use std::path::{Path, PathBuf};

use novel_state::Database;

fn outline_path(project_root: &Path) -> PathBuf {
    project_root.join("knowledge/plot/大纲.md")
}

fn parse_markdown_table_cells(line: &str) -> Option<Vec<String>> {
    if !line.starts_with('|') || line.contains("---") {
        return None;
    }
    let cells: Vec<String> = line.split('|').map(|s| s.trim().to_string()).collect();
    if cells.is_empty() {
        None
    } else {
        Some(cells)
    }
}

fn is_outline_table_header_cell(first: &str) -> bool {
    matches!(
        first,
        "卷" | "章" | "章节" | "世界" | "副本" | "版本" | "所在世界" | "volume" | "chapter"
    )
}

fn chapter_column_index(cells: &[String]) -> Option<usize> {
    cells.iter().position(|c| {
        let key = c.to_lowercase();
        key == "章" || key == "章节" || key == "chapter"
    })
}

fn max_chapter_from_outline_content(content: &str) -> Option<u32> {
    let mut max_ch = 0u32;
    let mut chapter_idx: Option<usize> = None;
    for line in content.lines() {
        let Some(cells) = parse_markdown_table_cells(line) else {
            continue;
        };
        let first = first_table_cell(&cells).unwrap_or("");
        if is_outline_table_header_cell(first) {
            chapter_idx = chapter_column_index(&cells);
            continue;
        }
        let idx = chapter_idx.unwrap_or(1);
        if let Some(ch) = cells.get(idx).and_then(|c| c.parse::<u32>().ok()) {
            max_ch = max_ch.max(ch);
        }
    }
    (max_ch > 0).then_some(max_ch)
}

fn last_h2_heading(content: &str) -> Option<String> {
    content.lines().rev().find_map(|line| {
        let line = line.trim();
        line.starts_with("## ")
            .then(|| line.trim_start_matches('#').trim().to_string())
    })
}

fn is_unit_table_header_cell(first: &str) -> bool {
    matches!(first, "卷" | "章节" | "世界")
}

fn first_table_cell(cells: &[String]) -> Option<&str> {
    cells.iter().find_map(|c| {
        let t = c.trim();
        (!t.is_empty()).then_some(t)
    })
}

fn count_unit_rows_in_section(content: &str, unit_heading: &str) -> u32 {
    let unit_lower = unit_heading.to_lowercase();
    let mut in_target_unit = false;
    let mut count = 0u32;
    for line in content.lines() {
        let line = line.trim();
        if line.starts_with("## ") {
            let heading = line.trim_start_matches('#').trim();
            in_target_unit = heading.to_lowercase() == unit_lower;
            continue;
        }
        if !in_target_unit {
            continue;
        }
        let Some(cells) = parse_markdown_table_cells(line) else {
            continue;
        };
        let first = first_table_cell(&cells).unwrap_or("");
        if !is_unit_table_header_cell(first) {
            count += 1;
        }
    }
    count
}

fn count_chapter_files(chapters_dir: &Path) -> u32 {
    if !chapters_dir.exists() {
        return 0;
    }
    std::fs::read_dir(chapters_dir)
        .ok()
        .map(|entries| {
            entries
                .filter_map(|e| e.ok())
                .filter(|e| e.path().extension().and_then(|x| x.to_str()) == Some("md"))
                .count() as u32
        })
        .unwrap_or(0)
}

pub(crate) fn outline_chapter_count(project_root: &Path) -> Option<u32> {
    let content = std::fs::read_to_string(outline_path(project_root)).ok()?;
    max_chapter_from_outline_content(&content)
}

/// Build progress summary for system prompt dynamic layer.
pub fn load_progress(project_root: &Path, session_id: &str, db: &Database) -> String {
    let chapters_dir = project_root.join("chapters");
    let completed = count_chapter_files(&chapters_dir);
    let next = completed + 1;
    let total = outline_chapter_count(project_root);
    let unit_context = current_structure_unit(project_root);
    let todos = match db.list_session_todos(session_id) {
        Ok(t) => t,
        Err(e) => {
            tracing::warn!(%session_id, %e, "failed to load session todos for progress");
            Vec::new()
        }
    };
    let in_progress: Vec<_> = todos
        .iter()
        .filter(|t| t.status == "in_progress")
        .map(|t| t.content.as_str())
        .collect();
    let mut lines = vec![
        format!("已完成章节文件: {completed}"),
        format!("下一章建议: Chapter {next}"),
    ];
    if let Some(t) = total {
        lines.push(format!("大纲计划章数: {t}"));
    }
    if !unit_context.is_empty() {
        lines.push(unit_context);
    }
    if !in_progress.is_empty() {
        lines.push(format!("进行中任务: {}", in_progress.join("; ")));
    }
    lines.join("\n")
}

pub(crate) fn current_structure_unit(project_root: &Path) -> String {
    let content = match std::fs::read_to_string(outline_path(project_root)) {
        Ok(c) => c,
        Err(_) => return String::new(),
    };
    let Some(last_heading) = last_h2_heading(&content) else {
        return String::new();
    };
    let unit_chapter_count = count_unit_rows_in_section(&content, &last_heading);
    if unit_chapter_count > 0 {
        format!("{last_heading}（本单元计划 {unit_chapter_count} 章）")
    } else {
        last_heading
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use novel_state::Database;
    use tempfile::TempDir;

    fn write_outline(tmp: &TempDir, content: &str) {
        std::fs::create_dir_all(tmp.path().join("knowledge/plot")).expect("dir");
        std::fs::write(tmp.path().join("knowledge/plot/大纲.md"), content).expect("w");
    }

    #[test]
    fn max_chapter_from_outline_parses_table() {
        let content = "|章 | 标题 |
|----|------|
| 1 | a |
| 3 | b |
";
        assert_eq!(max_chapter_from_outline_content(content), Some(3));
    }

    #[test]
    fn outline_chapter_count_from_table() {
        let tmp = TempDir::new().expect("tmp");
        write_outline(
            &tmp,
            "|章 | 标题 |
|----|------|
| 1 | a |
| 3 | b |
",
        );
        assert_eq!(outline_chapter_count(tmp.path()), Some(3));
    }

    #[test]
    fn current_structure_unit_reads_last_heading() {
        let tmp = TempDir::new().expect("tmp");
        write_outline(
            &tmp,
            "# 总纲
## 第一卷
content
## 第二卷
",
        );
        let unit = current_structure_unit(tmp.path());
        assert!(unit.contains("第二卷"));
    }

    #[test]
    fn current_structure_unit_counts_rows_in_section() {
        let tmp = TempDir::new().expect("tmp");
        write_outline(
            &tmp,
            "## 第二卷
|卷 | 章 |
|----|----|
| 2 | 1 |
| 2 | 2 |
",
        );
        let unit = current_structure_unit(tmp.path());
        assert!(unit.contains("本单元计划 2 章"));
    }

    #[test]
    fn load_progress_counts_chapters() {
        let tmp = TempDir::new().expect("tmp");
        std::fs::create_dir_all(tmp.path().join("chapters")).expect("dir");
        std::fs::write(tmp.path().join("chapters/chapter-001.md"), "x").expect("w");
        let db = Database::open(tmp.path().join("t.db")).expect("db");
        let sid = db
            .create_session(tmp.path().to_str().unwrap(), "m")
            .expect("s");
        let p = load_progress(tmp.path(), &sid, &db);
        assert!(p.contains("已完成章节文件: 1"));
    }
}
