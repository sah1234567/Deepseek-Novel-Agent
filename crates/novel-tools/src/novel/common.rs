use novel_knowledge::KnowledgeStore;
use regex::Regex;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::OnceLock;

/// Column index map built from the outline table header row.
pub type OutlineColumnMap = HashMap<String, usize>;

/// Build a ColumnMap from a header row. Recognizes standard column names.
pub fn build_outline_column_map(header_cells: &[String]) -> OutlineColumnMap {
    let mut map = HashMap::new();
    for (i, cell) in header_cells.iter().enumerate() {
        let key = cell.trim().to_lowercase();
        if !key.is_empty() {
            map.insert(key, i);
        }
    }
    map
}

/// Try to get the value of a column by name from a parsed row using the ColumnMap.
pub fn outline_col_value<'a>(
    cells: &'a [String],
    map: &OutlineColumnMap,
    name: &str,
) -> Option<&'a str> {
    map.get(name).and_then(|&i| cells.get(i)).map(|s| s.as_str())
}

/// Detect if a row looks like a header (first cell is a known column label).
pub fn is_outline_header_row(cells: &[String]) -> bool {
    if let Some(first) = cells.first() {
        let key = first.trim().to_lowercase();
        matches!(
            key.as_str(),
            "卷" | "章" | "章节" | "世界" | "副本" | "版本" | "所在世界" | "volume" | "chapter"
        )
    } else {
        false
    }
}

/// Parse the outline table from raw content. Returns header column map and data rows.
/// Also extracts the current structure unit from `## heading` markers.
pub fn parse_outline_with_header(
    content: &str,
) -> (Option<OutlineColumnMap>, String, Vec<Vec<String>>) {
    let mut colmap: Option<OutlineColumnMap> = None;
    let mut current_unit = String::new();
    for raw_line in content.lines() {
        let line = raw_line.trim();
        if line.starts_with("## ") {
            current_unit = line.trim_start_matches('#').trim().to_string();
        }
    }
    let all_rows = parse_table_rows(content);
    let mut data_rows = Vec::new();
    for cells in all_rows {
        if is_outline_header_row(&cells) {
            colmap = Some(build_outline_column_map(&cells));
            continue;
        }
        data_rows.push(cells);
    }
    (colmap, current_unit, data_rows)
}

/// Default fallback column map matching the standard 8-column outline format.
pub fn default_outline_column_map() -> &'static OutlineColumnMap {
    static MAP: OnceLock<OutlineColumnMap> = OnceLock::new();
    MAP.get_or_init(|| {
        let mut m = HashMap::new();
        m.insert("卷".into(), 0);
        m.insert("章".into(), 1);
        m.insert("章节标题".into(), 2);
        m.insert("核心事件".into(), 3);
        m.insert("需推进的伏笔".into(), 4);
        m.insert("张力".into(), 5);
        m.insert("pov".into(), 6);
        m.insert("时间点".into(), 7);
        m
    })
}

fn chapter_num_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| {
        Regex::new(r"(?i)chapter[-_]?(\d+)|Ch(\d+)|第(\d+)章").expect("valid chapter regex")
    })
}

fn table_row_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| {
        Regex::new(r"(?m)^\|([^|\n]+(?:\|[^|\n]+)*)\|$").expect("valid table row regex")
    })
}

/// Parse chapter number from paths like `chapter-031.md`, `Ch31`, `第31章`.
pub fn parse_chapter_num(s: &str) -> u32 {
    chapter_num_re()
        .captures(s)
        .and_then(|c| {
            c.get(1)
                .or_else(|| c.get(2))
                .or_else(|| c.get(3))
                .and_then(|m| m.as_str().parse().ok())
        })
        .unwrap_or(0)
}

pub fn list_character_names(store: &KnowledgeStore) -> Vec<String> {
    let chars_dir = store.root.join("knowledge/characters");
    if !chars_dir.exists() {
        return vec![];
    }
    let mut names = Vec::new();
    for entry in std::fs::read_dir(&chars_dir)
        .into_iter()
        .flatten()
        .flatten()
    {
        let name = entry.file_name().to_string_lossy().to_string();
        if name.ends_with(".md") && !name.starts_with('_') {
            names.push(name.trim_end_matches(".md").to_string());
        }
    }
    names.sort();
    names
}

pub fn parse_table_rows(content: &str) -> Vec<Vec<String>> {
    let re = table_row_re();
    let mut rows = Vec::new();
    for cap in re.captures_iter(content) {
        let cells: Vec<String> = cap
            .get(1)
            .map(|m| m.as_str())
            .unwrap_or("")
            .split('|')
            .map(|s| s.trim().to_string())
            .collect();
        if cells.is_empty() || cells[0].starts_with("---") || cells[0].contains("---") {
            continue;
        }
        rows.push(cells);
    }
    rows
}

pub fn list_chapter_files(root: &Path) -> Vec<(u32, PathBuf)> {
    let chapters_dir = root.join("chapters");
    if !chapters_dir.exists() {
        return vec![];
    }
    let mut out = Vec::new();
    for entry in std::fs::read_dir(&chapters_dir)
        .into_iter()
        .flatten()
        .flatten()
    {
        let path = entry.path();
        if path.extension().and_then(|e| e.to_str()) != Some("md") {
            continue;
        }
        let Some(file_name) = path.file_name() else { continue; };
        let num = parse_chapter_num(&file_name.to_string_lossy());
        if num > 0 {
            out.push((num, path));
        }
    }
    out.sort_by_key(|(n, _)| *n);
    out
}

pub fn list_outline_files(root: &Path) -> Vec<(u32, PathBuf)> {
    let dir = root.join("knowledge/plot/细纲");
    if !dir.exists() {
        return vec![];
    }
    let mut out = Vec::new();
    for entry in std::fs::read_dir(&dir).into_iter().flatten().flatten() {
        let path = entry.path();
        if path.extension().and_then(|e| e.to_str()) != Some("md") {
            continue;
        }
        let Some(file_name) = path.file_name() else { continue; };
        let num = parse_chapter_num(&file_name.to_string_lossy());
        if num > 0 {
            out.push((num, path));
        }
    }
    out.sort_by_key(|(n, _)| *n);
    out
}

pub fn in_chapter_range(num: u32, range: Option<(u32, u32)>) -> bool {
    match range {
        None => true,
        Some((start, end)) => num >= start && num <= end,
    }
}

pub fn count_chinese_chars(text: &str) -> u32 {
    text.chars()
        .filter(|c| {
            matches!(*c as u32,
                0x4E00..=0x9FFF |   // CJK Unified Ideographs (common)
                0x3400..=0x4DBF |   // CJK Extension A
                0x20000..=0x2A6DF   // CJK Extension B
            )
        })
        .count() as u32
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_chapter_variants() {
        assert_eq!(parse_chapter_num("chapter-031.md"), 31);
        assert_eq!(parse_chapter_num("Ch5"), 5);
        assert_eq!(parse_chapter_num("第12章"), 12);
    }
}
