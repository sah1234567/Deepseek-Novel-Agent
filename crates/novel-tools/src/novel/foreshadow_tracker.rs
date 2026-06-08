use super::common::parse_chapter_num;
use crate::{require_str, Tool, ToolContext, ToolError, ToolOutput};
use async_trait::async_trait;
use novel_knowledge::KnowledgeStore;
use serde_json::{json, Value};

pub struct ForeshadowTrackerTool;

fn parse_pending_foreshadows(content: &str) -> Vec<(String, String, String, u32)> {
    let mut pending: std::collections::HashMap<String, (String, String, u32)> =
        std::collections::HashMap::new();
    // Only scan the first markdown table in the file; stop at next `## ` heading or EOF.
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

fn build_output(
    pending: Vec<(String, String, String, u32)>,
    current_num: u32,
    threshold: i32,
    header: &str,
) -> String {
    let mut overdue: Vec<String> = Vec::new();
    let mut urgent: Vec<String> = Vec::new();
    let mut upcoming: Vec<String> = Vec::new();

    for (_id, row, _expected, expected_num) in pending {
        if expected_num == 0 {
            upcoming.push(row);
            continue;
        }
        let distance = expected_num as i32 - current_num as i32;
        if distance < 0 {
            let late = (-distance) as u32;
            overdue.push(format!("{row}  [overdue by {late} chapters]"));
        } else if distance <= threshold {
            urgent.push(format!("{row}  [due in {distance} chapters]"));
        } else {
            upcoming.push(format!("{row}  [due in {distance} chapters]"));
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

#[async_trait]
impl Tool for ForeshadowTrackerTool {
    fn name(&self) -> &str {
        "ForeshadowTracker"
    }
    fn description(&self) -> &str {
        "Track pending foreshadowings — returns categorized markdown (overdue/urgent/upcoming) with distance annotations"
    }
    fn input_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "current_chapter": {"type": "string"},
                "warning_threshold": {"type": "integer", "default": 5},
                "character": {"type": "string", "description": "Filter foreshadows by associated character name"}
            },
            "required": ["current_chapter"]
        })
    }
    fn is_read_only(&self) -> bool {
        true
    }

    async fn call(&self, input: Value, ctx: &ToolContext) -> Result<ToolOutput, ToolError> {
        let current_chapter = require_str(&input, "current_chapter")?;
        let current_num = parse_chapter_num(&current_chapter);
        let threshold = input
            .get("warning_threshold")
            .and_then(|v| v.as_u64())
            .unwrap_or(5) as i32;
        let filter_character = input.get("character").and_then(|v| v.as_str());

        let store = KnowledgeStore::new(&ctx.project_root);
        let content = store
            .read_file("knowledge/plot/伏笔追踪.md")
            .unwrap_or_default();
        // Extract column header for output readability.
        let header = content
            .lines()
            .find(|l| l.contains("章节") && l.contains("伏笔ID"))
            .unwrap_or("")
            .trim()
            .to_string();
        let mut pending = parse_pending_foreshadows(&content);

        if let Some(ch) = filter_character {
            let ch_lower = ch.to_lowercase();
            pending.retain(|(_id, row, _expected, _num)| {
                // Check cells[7] (关联人物) of this row, not the whole file.
                let cells: Vec<&str> = row.split('|').map(|s| s.trim()).collect();
                cells
                    .get(7)
                    .map(|c| c.to_lowercase().contains(&ch_lower))
                    .unwrap_or(false)
            });
        }

        let output = build_output(pending, current_num, threshold, &header);
        Ok(ToolOutput {
            content: output,
            is_error: false,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{PermissionMode, ToolContext};
    use tempfile::TempDir;

    fn write_foreshadow(root: &std::path::Path, body: &str) {
        std::fs::create_dir_all(root.join("knowledge/plot")).unwrap();
        std::fs::write(root.join("knowledge/plot/伏笔追踪.md"), body).unwrap();
    }

    #[test]
    fn build_result_marks_overdue() {
        let pending = vec![(
            "F01".into(),
            "| Ch5 | F01 | 埋设 | 伤疤 | 待回收 | Ch5 | 陈默 |".into(),
            "Ch5".into(),
            5u32,
        )];
        let result = build_output(pending, 10, 5, "");
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
        let result = build_output(pending, 1, 5, "");
        assert!(!result.contains("Overdue"));
        assert!(result.contains("Upcoming"));
    }

    #[tokio::test(flavor = "current_thread")]
    async fn foreshadow_chapter1_no_overdue() {
        let tmp = TempDir::new().unwrap();
        write_foreshadow(
            tmp.path(),
            "| 章节 | 伏笔ID | 操作 | 内容描述 | 状态 | 预计回收章 | 关联人物 |\n\
             |------|--------|------|---------|------|-----------|----------|\n\
             | Ch5 | F01 | 埋设 | 伤疤发光 | 待回收 | Ch35 | 陈默 |\n",
        );
        let tool = ForeshadowTrackerTool;
        let ctx = ToolContext {
            permission_mode: PermissionMode::Auto,
            project_root: tmp.path().to_path_buf(),
            ..ToolContext::new(tmp.path().to_path_buf())
        };
        let out = tool
            .call(json!({"current_chapter": "Ch1"}), &ctx)
            .await
            .unwrap();
        assert!(!out.content.contains("Overdue"));
        assert!(out.content.contains("Upcoming"));
        assert!(
            !out.content.starts_with('{'),
            "should return markdown, not JSON"
        );
    }

    #[tokio::test(flavor = "current_thread")]
    async fn character_filter_matches_associated_column_only() {
        let tmp = TempDir::new().unwrap();
        write_foreshadow(
            tmp.path(),
            "| 章节 | 伏笔ID | 操作 | 内容描述 | 状态 | 预计回收章 | 关联人物 |\n\
             |------|--------|------|---------|------|-----------|----------|\n\
             | Ch5 | F01 | 埋设 | 陈默向苏婉清提起林若烟 | 待回收 | Ch10 | 陈默 |\n\
             | Ch6 | F02 | 埋设 | 苏婉清的秘密 | 待回收 | Ch12 | 林若烟 |\n",
        );
        let tool = ForeshadowTrackerTool;
        let ctx = ToolContext {
            permission_mode: PermissionMode::Auto,
            project_root: tmp.path().to_path_buf(),
            ..ToolContext::new(tmp.path().to_path_buf())
        };
        let out = tool
            .call(
                json!({"current_chapter": "Ch1", "character": "苏婉清"}),
                &ctx,
            )
            .await
            .unwrap();
        assert!(
            out.content.contains("无待回收伏笔"),
            "苏婉清仅出现在内容描述列，不应匹配: {}",
            out.content
        );
        let out = tool
            .call(json!({"current_chapter": "Ch1", "character": "陈默"}), &ctx)
            .await
            .unwrap();
        assert!(out.content.contains("F01"));
        assert!(!out.content.contains("F02"));
    }

    #[tokio::test(flavor = "current_thread")]
    async fn foreshadow_all_recovered_empty() {
        let tmp = TempDir::new().unwrap();
        write_foreshadow(
            tmp.path(),
            "| 章节 | 伏笔ID | 操作 | 内容描述 | 状态 | 预计回收章 | 关联人物 |\n\
             |------|--------|------|---------|------|-----------|----------|\n\
             | Ch28 | F01 | 回收 | 已解 | 已回收 | — | 陈默 |\n",
        );
        let tool = ForeshadowTrackerTool;
        let ctx = ToolContext {
            permission_mode: PermissionMode::Auto,
            project_root: tmp.path().to_path_buf(),
            ..ToolContext::new(tmp.path().to_path_buf())
        };
        let out = tool
            .call(json!({"current_chapter": "Ch35"}), &ctx)
            .await
            .unwrap();
        assert!(out.content.contains("无待回收伏笔"));
        assert!(!out.content.contains("Overdue"));
        assert!(!out.content.starts_with('{'));
    }
}
