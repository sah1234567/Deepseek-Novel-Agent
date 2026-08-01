//! Foreshadow tracking table parsing, categorization, output, and digest derivation.
//!
//! Pure functions — single source shared by the `ForeshadowTracker` tool
//! (novel-tools) and the Progress-section injection (novel-core). Moved here
//! from novel-tools so the leaf crate owns table semantics
//! (docs/crates/novel-knowledge.md §1.1.2).

use crate::KnowledgeStore;
use regex::Regex;
use std::sync::OnceLock;

/// Parse chapter number from strings like `chapter-031.md`, `Ch31`, `第31章`.
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

fn chapter_num_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(
        || match Regex::new(r"(?i)chapter[-_]?(\d+)|Ch(\d+)|第(\d+)章") {
            Ok(re) => re,
            Err(e) => panic!("chapter regex: {e}"),
        },
    )
}

/// Parse pending foreshadows from the tracking table content.
///
/// Only scans the first markdown table in the file; stops at the next `## ` heading
/// or EOF. Rows with fewer than 8 cells are skipped. Status is matched by substring:
/// `待回收` → pending; `已回收` / `已废弃` → removed from pending.
pub fn parse_pending_foreshadows(content: &str) -> Vec<(String, String, String, u32)> {
    let mut pending: std::collections::HashMap<String, (String, String, u32)> =
        std::collections::HashMap::new();
    let table_start = content.find('|').unwrap_or(content.len());
    let table_section = &content[table_start..];
    let table_end = table_section.find("\n## ").unwrap_or(table_section.len());
    for line in table_section[..table_end].lines() {
        if !line.starts_with('|') || line.contains("章节") || line.contains("---") {
            continue;
        }
        let cells: Vec<&str> = line.split('|').map(|s| s.trim()).collect();
        if cells.len() < 8 {
            continue;
        }
        let id = cells[2].to_string();
        let status = cells[5];
        let expected = cells[6].to_string();
        if status.contains("待回收") {
            let expected_num = parse_chapter_num(&expected);
            pending.insert(id, (line.to_string(), expected, expected_num));
        } else if status.contains("已回收") || status.contains("已废弃") {
            pending.remove(&id);
        }
    }
    pending
        .into_iter()
        .map(|(id, (row, expected, num))| (id, row, expected, num))
        .collect()
}

/// One pending foreshadow, categorized by recovery deadline.
pub struct CategorizedForeshadow {
    pub id: String,
    pub row: String,
    pub expected_num: u32,
}

/// Pending foreshadows split into the three deadline buckets.
pub struct CategorizedForeshadows {
    pub overdue: Vec<CategorizedForeshadow>,
    pub urgent: Vec<CategorizedForeshadow>,
    pub upcoming: Vec<CategorizedForeshadow>,
}

/// Split pending foreshadows by distance to `current_num` (threshold inclusive → urgent).
pub fn categorize_foreshadows(
    pending: Vec<(String, String, String, u32)>,
    current_num: u32,
    threshold: i32,
) -> CategorizedForeshadows {
    let mut overdue = Vec::new();
    let mut urgent = Vec::new();
    let mut upcoming = Vec::new();
    for (id, row, _expected, expected_num) in pending {
        if expected_num == 0 {
            upcoming.push(CategorizedForeshadow {
                id,
                row,
                expected_num,
            });
            continue;
        }
        let distance = expected_num as i32 - current_num as i32;
        if distance < 0 {
            overdue.push(CategorizedForeshadow {
                id,
                row,
                expected_num,
            });
        } else if distance <= threshold {
            urgent.push(CategorizedForeshadow {
                id,
                row,
                expected_num,
            });
        } else {
            upcoming.push(CategorizedForeshadow {
                id,
                row,
                expected_num,
            });
        }
    }
    CategorizedForeshadows {
        overdue,
        urgent,
        upcoming,
    }
}

/// Format the categorized markdown report (ForeshadowTracker tool output).
///
/// Output shape is locked by tests — do not change formatting.
pub fn build_foreshadow_output(
    cat: &CategorizedForeshadows,
    current_num: u32,
    threshold: i32,
    header: &str,
) -> String {
    let mut overdue: Vec<String> = Vec::new();
    let mut urgent: Vec<String> = Vec::new();
    let mut upcoming: Vec<String> = Vec::new();

    for f in &cat.overdue {
        // overdue guarantees current_num > expected_num, so this is never 0.
        let late = current_num.saturating_sub(f.expected_num);
        overdue.push(format!("{}  [overdue by {late} chapters]", f.row));
    }
    for f in &cat.urgent {
        let distance = f.expected_num as i32 - current_num as i32;
        urgent.push(format!("{}  [due in {distance} chapters]", f.row));
    }
    for f in &cat.upcoming {
        if f.expected_num == 0 {
            upcoming.push(f.row.clone());
        } else {
            let distance = f.expected_num as i32 - current_num as i32;
            upcoming.push(format!("{}  [due in {distance} chapters]", f.row));
        }
    }

    let has_overdue = !overdue.is_empty();
    let has_urgent = !urgent.is_empty();
    let has_upcoming = !upcoming.is_empty();
    let mut lines = Vec::new();

    if has_overdue {
        lines.push(format!(
            "## Overdue ({} overdue, current=Ch{current_num})\n\n{header}",
            overdue.len()
        ));
        lines.extend(overdue);
        lines.push(String::new());
    }

    if has_urgent {
        lines.push(format!(
            "## Urgent ({} within {threshold} chapters)\n\n{header}",
            urgent.len()
        ));
        lines.extend(urgent);
        lines.push(String::new());
    }

    if has_upcoming {
        lines.push(format!(
            "## Upcoming ({} beyond {threshold} chapters)\n\n{header}",
            upcoming.len()
        ));
        lines.extend(upcoming);
        lines.push(String::new());
    }

    if !has_overdue && !has_urgent && !has_upcoming {
        lines.push("(无待回收伏笔)".to_string());
    }

    lines.join("\n").trim().to_string()
}

/// Derive a compact "active foreshadow" digest (≤500 chars) for Progress injection.
///
/// `None` when the tracking file is missing or there are no pending foreshadows
/// (an empty table must not add noise to the Progress section).
pub fn derive_foreshadow_digest(store: &KnowledgeStore, current_chapter: &str) -> Option<String> {
    let content = store.read_file("knowledge/plot/伏笔追踪.md").ok()?;
    let current_num = parse_chapter_num(current_chapter);
    let cat = categorize_foreshadows(parse_pending_foreshadows(&content), current_num, 5);

    let mut parts = Vec::new();
    if !cat.overdue.is_empty() {
        let ids: Vec<&str> = cat.overdue.iter().map(|f| f.id.as_str()).collect();
        parts.push(format!(
            "overdue {} ({})",
            cat.overdue.len(),
            ids.join(", ")
        ));
    }
    if !cat.urgent.is_empty() {
        let ids: Vec<&str> = cat.urgent.iter().map(|f| f.id.as_str()).collect();
        parts.push(format!("urgent {} ({})", cat.urgent.len(), ids.join(", ")));
    }
    if !cat.upcoming.is_empty() {
        parts.push(format!("upcoming {}", cat.upcoming.len()));
    }
    if parts.is_empty() {
        return None;
    }
    Some(format!("活跃伏笔: {}", parts.join(" | ")))
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn parse_chapter_num_variants() {
        assert_eq!(parse_chapter_num("chapter-031.md"), 31);
        assert_eq!(parse_chapter_num("Ch5"), 5);
        assert_eq!(parse_chapter_num("第12章"), 12);
        assert_eq!(parse_chapter_num("—"), 0);
        assert_eq!(parse_chapter_num(""), 0);
    }

    #[test]
    fn build_output_marks_overdue() {
        let pending = vec![(
            "F01".into(),
            "| Ch5 | F01 | 埋设 | 伤疤 | 待回收 | Ch5 | 陈默 |".into(),
            "Ch5".into(),
            5u32,
        )];
        let cat = categorize_foreshadows(pending, 10, 5);
        let result = build_foreshadow_output(&cat, 10, 5, "");
        assert!(result.contains("Overdue"), "should mark as overdue");
        assert!(result.contains("overdue by 5 chapters"));
    }

    #[test]
    fn far_future_in_upcoming_not_dropped() {
        let pending = vec![(
            "F01".into(),
            "| Ch1 | F01 | 埋设 | 伤疤 | 待回收 | Ch35 | 陈默 |".into(),
            "Ch35".into(),
            35u32,
        )];
        let cat = categorize_foreshadows(pending, 1, 5);
        let result = build_foreshadow_output(&cat, 1, 5, "");
        assert!(!result.contains("Overdue"));
        assert!(result.contains("Upcoming"));
    }

    fn write_foreshadow(root: &std::path::Path, body: &str) {
        std::fs::create_dir_all(root.join("knowledge/plot")).unwrap();
        std::fs::write(root.join("knowledge/plot/伏笔追踪.md"), body).unwrap();
    }

    fn digest_for(body: &str, current: &str) -> Option<String> {
        let tmp = TempDir::new().unwrap();
        write_foreshadow(tmp.path(), body);
        let store = KnowledgeStore::new(tmp.path());
        derive_foreshadow_digest(&store, current)
    }

    #[test]
    fn digest_lists_overdue_and_urgent_ids() {
        let body = "| 章节 | 伏笔ID | 操作 | 内容描述 | 状态 | 预计回收章 | 关联人物 |\n\
                    |------|--------|------|---------|------|-----------|----------|\n\
                    | Ch5 | F01 | 埋设 | 伤疤发光 | 待回收 | Ch3 | 陈默 |\n\
                    | Ch6 | F02 | 埋设 | 玉佩 | 待回收 | Ch9 | 苏雨桐 |\n\
                    | Ch7 | F03 | 埋设 | 书信 | 待回收 | Ch30 | 林若烟 |\n";
        let d = digest_for(body, "Ch8").expect("digest");
        assert!(d.contains("overdue 1 (F01)"), "got: {d}");
        assert!(d.contains("urgent 1 (F02)"), "got: {d}");
        assert!(d.contains("upcoming 1"), "got: {d}");
    }

    #[test]
    fn digest_none_when_all_recovered_or_missing() {
        let body = "| 章节 | 伏笔ID | 操作 | 内容描述 | 状态 | 预计回收章 | 关联人物 |\n\
                    |------|--------|------|---------|------|-----------|----------|\n\
                    | Ch28 | F01 | 回收 | 已解 | 已回收 | — | 陈默 |\n";
        assert!(digest_for(body, "Ch35").is_none());
        // Missing file → None (no noise in Progress).
        let tmp = TempDir::new().unwrap();
        let store = KnowledgeStore::new(tmp.path());
        assert!(derive_foreshadow_digest(&store, "Ch1").is_none());
    }
}
