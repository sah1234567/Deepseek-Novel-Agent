use crate::{require_str, Tool, ToolContext, ToolError, ToolOutput};
use async_trait::async_trait;
use novel_knowledge::{
    build_foreshadow_output, categorize_foreshadows, parse_chapter_num, parse_pending_foreshadows,
    KnowledgeStore,
};
use serde_json::{json, Value};

pub struct ForeshadowTrackerTool;

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

        let cat = categorize_foreshadows(pending, current_num, threshold);
        let output = build_foreshadow_output(&cat, current_num, threshold, &header);
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
