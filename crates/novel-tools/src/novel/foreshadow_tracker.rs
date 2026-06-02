use super::common::parse_chapter_num;
use crate::{require_str, Tool, ToolContext, ToolError, ToolOutput};
use async_trait::async_trait;
use novel_knowledge::KnowledgeStore;
use serde::Serialize;
use serde_json::{json, Value};

#[derive(Debug, Serialize)]
struct ForeshadowEntry {
    id: String,
    content: String,
    expected_chapter: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    distance: Option<i32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    overdue_by: Option<u32>,
}

#[derive(Debug, Serialize)]
pub(crate) struct ForeshadowTrackerResult {
    urgent: Vec<ForeshadowEntry>,
    overdue: Vec<ForeshadowEntry>,
    upcoming: Vec<ForeshadowEntry>,
    density_warning: Option<String>,
}

pub struct ForeshadowTrackerTool;

fn parse_pending_foreshadows(content: &str) -> Vec<(String, String, String, u32)> {
    let mut pending: std::collections::HashMap<String, (String, String, u32)> =
        std::collections::HashMap::new();
    for line in content.lines() {
        if !line.starts_with('|') || line.contains("章节") || line.contains("---") {
            continue;
        }
        let cells: Vec<&str> = line.split('|').map(|s| s.trim()).collect();
        if cells.len() < 8 {
            continue;
        }
        let id = cells[2].to_string();
        let desc = cells[4].to_string();
        let status = cells[5];
        let expected = cells[6].to_string();
        if status.contains("待回收") {
            let expected_num = parse_chapter_num(&expected);
            pending.insert(id, (desc, expected, expected_num));
        } else if status.contains("已回收") {
            pending.remove(&id);
        }
    }
    pending
        .into_iter()
        .map(|(id, (content, expected, num))| (id, content, expected, num))
        .collect()
}

pub(crate) fn build_foreshadow_tracker_result(
    pending: Vec<(String, String, String, u32)>,
    current_num: u32,
    threshold: i32,
) -> ForeshadowTrackerResult {
    let mut urgent = Vec::new();
    let mut overdue = Vec::new();
    let mut upcoming = Vec::new();

    for (id, desc, expected, expected_num) in pending {
        if expected_num == 0 {
            upcoming.push(ForeshadowEntry {
                id,
                content: desc,
                expected_chapter: expected,
                distance: None,
                overdue_by: None,
            });
            continue;
        }
        let distance = expected_num as i32 - current_num as i32;
        if distance < 0 {
            overdue.push(ForeshadowEntry {
                id: id.clone(),
                content: desc,
                expected_chapter: expected,
                distance: Some(distance),
                overdue_by: Some((-distance) as u32),
            });
        } else if distance == 0 {
            urgent.push(ForeshadowEntry {
                id,
                content: desc,
                expected_chapter: expected,
                distance: Some(0),
                overdue_by: None,
            });
        } else if distance <= threshold {
            urgent.push(ForeshadowEntry {
                id,
                content: desc,
                expected_chapter: expected,
                distance: Some(distance),
                overdue_by: None,
            });
        } else if distance <= threshold * 2 {
            upcoming.push(ForeshadowEntry {
                id,
                content: desc,
                expected_chapter: expected,
                distance: Some(distance),
                overdue_by: None,
            });
        }
    }

    ForeshadowTrackerResult {
        urgent,
        overdue,
        upcoming,
        density_warning: None,
    }
}

#[async_trait]
impl Tool for ForeshadowTrackerTool {
    fn name(&self) -> &str {
        "ForeshadowTracker"
    }
    fn description(&self) -> &str {
        "Track pending foreshadowings and recovery deadlines"
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
        let mut pending = parse_pending_foreshadows(&content);

        if let Some(ch) = filter_character {
            pending.retain(|(id, _desc, _expected, _num)| {
                content
                    .lines()
                    .any(|line| line.contains(id) && line.contains(ch))
            });
        }

        let result = build_foreshadow_tracker_result(pending, current_num, threshold);
        Ok(ToolOutput {
            content: serde_json::to_string_pretty(&result)
                .map_err(|e| ToolError::Execution(format!("json serialize: {e}")))?,
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
        let pending = vec![("F01".into(), "伤疤".into(), "Ch5".into(), 5u32)];
        let result = build_foreshadow_tracker_result(pending, 10, 5);
        assert_eq!(result.overdue.len(), 1);
        assert_eq!(result.overdue[0].overdue_by, Some(5));
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
        assert!(!out.content.contains("overdue_by"));
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
        assert!(out.content.contains("\"urgent\": []"));
        assert!(out.content.contains("\"overdue\": []"));
    }
}
