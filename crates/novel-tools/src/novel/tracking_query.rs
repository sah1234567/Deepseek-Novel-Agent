use super::common::parse_chapter_num;
use crate::{require_str, Tool, ToolContext, ToolError, ToolOutput};
use async_trait::async_trait;
use novel_knowledge::KnowledgeStore;
use serde::Serialize;
use serde_json::{json, Value};

const FILE_MAP: &[(&str, &str)] = &[
    ("scene", "knowledge/shared-systems/场景追踪.md"),
    ("prop", "knowledge/shared-systems/道具追踪.md"),
    ("faction", "knowledge/shared-systems/势力追踪.md"),
    ("timeline", "knowledge/shared-systems/时间线.md"),
    ("power", "knowledge/shared-systems/战力系统.md"),
    ("ability", "knowledge/shared-systems/功法技能.md"),
];

#[derive(Debug, Serialize)]
struct TrackingEntry {
    chapter: String,
    content: String,
}

#[derive(Debug, Serialize)]
struct TrackingQueryResult {
    file: String,
    operation: String,
    entries: Vec<TrackingEntry>,
    #[serde(skip_serializing_if = "Option::is_none")]
    current_state: Option<String>,
}

fn resolve_file_path(file: &str) -> Result<&'static str, ToolError> {
    FILE_MAP
        .iter()
        .find(|(k, _)| *k == file)
        .map(|(_, p)| *p)
        .ok_or_else(|| {
            ToolError::Execution(format!(
                "unknown tracking file: {file}. Use: scene, prop, faction, timeline, power, ability"
            ))
        })
}

fn parse_table_rows(content: &str, table_heading: &str) -> Vec<String> {
    let heading = format!("## {table_heading}");
    let Some(section_start) = content.find(&heading) else {
        return vec![];
    };
    let section = &content[section_start..];
    let mut rows: Vec<String> = Vec::new();
    for line in section.lines().skip(2) {
        // skip heading and blank line after it
        if line.starts_with('|') && !line.contains("---") && !line.contains("章节") {
            rows.push(line.trim().to_string());
        }
    }
    rows
}

fn extract_current_state(rows: &[String]) -> Option<String> {
    rows.last().map(|r| r.to_string())
}

fn filter_by_chapter_range(rows: &[String], range: (u32, u32)) -> Vec<TrackingEntry> {
    rows.iter()
        .filter_map(|row| {
            let ch = parse_chapter_num(row);
            if ch >= range.0 && ch <= range.1 {
                Some(TrackingEntry {
                    chapter: format!("Ch{ch}"),
                    content: row.clone(),
                })
            } else {
                None
            }
        })
        .collect()
}

fn search_rows(rows: &[String], keyword: &str) -> Vec<TrackingEntry> {
    let kw = keyword.to_lowercase();
    rows.iter()
        .filter(|row| row.to_lowercase().contains(&kw))
        .map(|row| TrackingEntry {
            chapter: format!("Ch{}", parse_chapter_num(row)),
            content: row.clone(),
        })
        .collect()
}

fn parse_chapter_range(input: &Value) -> Option<(u32, u32)> {
    let arr = input.get("chapter_range")?.as_array()?;
    if arr.len() != 2 {
        return None;
    }
    Some((arr[0].as_u64()? as u32, arr[1].as_u64()? as u32))
}

pub struct TrackingQueryTool;

#[async_trait]
impl Tool for TrackingQueryTool {
    fn name(&self) -> &str {
        "TrackingQuery"
    }
    fn description(&self) -> &str {
        "Query scene/prop/faction/timeline/power/skill tracking tables — current state, chapter range, or keyword search"
    }
    fn input_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "file": {
                    "type": "string",
                    "enum": ["scene", "prop", "faction", "timeline", "power", "ability"],
                    "description": "Which tracking file to query"
                },
                "operation": {
                    "type": "string",
                    "enum": ["current", "range", "search"],
                    "description": "current=last row only, range=filter by chapter_range, search=keyword filter"
                },
                "chapter_range": {
                    "type": "array",
                    "items": {"type": "integer"},
                    "minItems": 2,
                    "maxItems": 2,
                    "description": "[start_ch, end_ch] for range operation"
                },
                "keyword": {
                    "type": "string",
                    "description": "Search term for search operation"
                }
            },
            "required": ["file", "operation"]
        })
    }
    fn is_read_only(&self) -> bool {
        true
    }

    async fn call(&self, input: Value, ctx: &ToolContext) -> Result<ToolOutput, ToolError> {
        let file = require_str(&input, "file")?;
        let operation = require_str(&input, "operation")?;
        let path = resolve_file_path(&file)?;
        let store = KnowledgeStore::new(&ctx.project_root);
        let content = store.read_file(path).unwrap_or_default();

        let table_heading = match file.as_str() {
            "scene" => "场景演变日志",
            "prop" => "道具演变日志",
            "faction" => "势力演变日志",
            "timeline" => "时间线演变日志",
            "power" => "战力演变日志",
            "ability" => "功法演变日志",
            _ => return Err(ToolError::Execution(format!("unknown file: {file}"))),
        };

        let rows = parse_table_rows(&content, table_heading);

        let (entries, current_state) = match operation.as_str() {
            "current" => {
                let state = extract_current_state(&rows);
                let entries = state
                    .iter()
                    .map(|r| TrackingEntry {
                        chapter: format!("Ch{}", parse_chapter_num(r)),
                        content: r.clone(),
                    })
                    .collect();
                (entries, state)
            }
            "range" => {
                let range = parse_chapter_range(&input).ok_or_else(|| {
                    ToolError::Execution("chapter_range required for range operation".into())
                })?;
                (filter_by_chapter_range(&rows, range), None)
            }
            "search" => {
                let keyword = require_str(&input, "keyword")?;
                (search_rows(&rows, &keyword), None)
            }
            _ => {
                return Err(ToolError::Execution(format!(
                    "unknown operation: {operation}"
                )))
            }
        };

        let result = TrackingQueryResult {
            file: file.to_string(),
            operation: operation.to_string(),
            entries,
            current_state,
        };

        Ok(ToolOutput {
            content: serde_json::to_string_pretty(&result)
                .map_err(|e| ToolError::Internal(e.to_string()))?,
            is_error: false,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::PermissionMode;
    use tempfile::TempDir;

    fn write_tracking(root: &std::path::Path, file: &str, body: &str) {
        let dir = root.join("knowledge/shared-systems");
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join(file), body).unwrap();
    }

    #[tokio::test(flavor = "current_thread")]
    async fn query_scene_current_state() {
        let tmp = TempDir::new().unwrap();
        write_tracking(
            tmp.path(),
            "场景追踪.md",
            "## 场景演变日志\n\
             | 章节 | 场景 | 状态 |\n\
             |------|------|------|\n\
             | Ch3 | 矿洞 | 已探索 |\n\
             | Ch5 | 矿洞 | 坍塌 |\n",
        );
        let tool = TrackingQueryTool;
        let ctx = ToolContext {
            permission_mode: PermissionMode::Auto,
            project_root: tmp.path().to_path_buf(),
            ..ToolContext::new(tmp.path().to_path_buf())
        };
        let out = tool
            .call(json!({"file": "scene", "operation": "current"}), &ctx)
            .await
            .unwrap();
        assert!(out.content.contains("坍塌"));
        assert!(out.content.contains("current_state"));
    }

    #[tokio::test(flavor = "current_thread")]
    async fn query_range_filters_by_chapter() {
        let tmp = TempDir::new().unwrap();
        write_tracking(
            tmp.path(),
            "道具追踪.md",
            "## 道具演变日志\n\
             | 章节 | 道具 | 状态 |\n\
             |------|------|------|\n\
             | Ch1 | 古戒 | 获得 |\n\
             | Ch3 | 古戒 | 觉醒 |\n\
             | Ch5 | 古戒 | 遗失 |\n\
             | Ch7 | 古戒 | 寻回 |\n",
        );
        let tool = TrackingQueryTool;
        let ctx = ToolContext {
            permission_mode: PermissionMode::Auto,
            project_root: tmp.path().to_path_buf(),
            ..ToolContext::new(tmp.path().to_path_buf())
        };
        let out = tool
            .call(
                json!({"file": "prop", "operation": "range", "chapter_range": [1, 3]}),
                &ctx,
            )
            .await
            .unwrap();
        assert!(out.content.contains("Ch1"));
        assert!(out.content.contains("Ch3"));
        assert!(!out.content.contains("Ch5"));
    }

    #[tokio::test(flavor = "current_thread")]
    async fn search_by_keyword() {
        let tmp = TempDir::new().unwrap();
        write_tracking(
            tmp.path(),
            "势力追踪.md",
            "## 势力演变日志\n\
             | 章节 | 势力 | 变化 |\n\
             |------|------|------|\n\
             | Ch2 | 青云宗 | 结盟 |\n\
             | Ch4 | 血月教 | 敌对 |\n\
             | Ch6 | 青云宗 | 背叛 |\n",
        );
        let tool = TrackingQueryTool;
        let ctx = ToolContext {
            permission_mode: PermissionMode::Auto,
            project_root: tmp.path().to_path_buf(),
            ..ToolContext::new(tmp.path().to_path_buf())
        };
        let out = tool
            .call(
                json!({"file": "faction", "operation": "search", "keyword": "青云宗"}),
                &ctx,
            )
            .await
            .unwrap();
        assert!(out.content.contains("结盟"));
        assert!(out.content.contains("背叛"));
        assert!(!out.content.contains("血月教"));
    }

    #[tokio::test(flavor = "current_thread")]
    async fn missing_file_returns_empty() {
        let tmp = TempDir::new().unwrap();
        let tool = TrackingQueryTool;
        let ctx = ToolContext {
            permission_mode: PermissionMode::Auto,
            project_root: tmp.path().to_path_buf(),
            ..ToolContext::new(tmp.path().to_path_buf())
        };
        let out = tool
            .call(json!({"file": "scene", "operation": "current"}), &ctx)
            .await
            .unwrap();
        assert!(out.content.contains("\"entries\": []"));
    }
}
