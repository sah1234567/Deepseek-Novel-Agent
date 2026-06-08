use super::common::parse_chapter_num;
use crate::{require_str, Tool, ToolContext, ToolError, ToolOutput};
use async_trait::async_trait;
use novel_knowledge::KnowledgeStore;
use serde::Serialize;
use serde_json::{json, Value};

#[derive(Debug, Clone, Serialize)]
struct RelationEntry {
    chapter: String,
    speaker: String,
    object: String,
    relation: String,
    calling_speaker_to_object: String,
    calling_object_to_speaker: String,
    event: String,
    #[serde(skip)]
    raw_row: String,
}

#[derive(Debug, Serialize)]
struct RelationQueryResult {
    character: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    target: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    current: Option<RelationEntry>,
    #[serde(skip_serializing_if = "Option::is_none")]
    history: Option<Vec<RelationEntry>>,
    all_relations: Vec<RelationEntry>,
}

fn parse_relation_table(content: &str) -> Vec<RelationEntry> {
    // Template: | 章节 | 说话者 | 对象 | 旧关系 | 新关系 | 说话者称呼变化 | 对方对说话者称呼 | 触发事件 |
    // cells[1]=章节  cells[2]=说话者  cells[3]=对象  cells[4]=旧关系  cells[5]=新关系
    // cells[6]=说话者称呼变化  cells[7]=对方对说话者称呼  cells[8]=触发事件
    let heading = "## 关系演变日志";
    let Some(section_start) = content.find(heading) else {
        return vec![];
    };
    let section = &content[section_start..];
    let mut entries = Vec::new();
    for line in section.lines().skip(2) {
        if !line.starts_with('|') || line.contains("---") || line.contains("章节") {
            continue;
        }
        let cells: Vec<&str> = line.split('|').map(|s| s.trim()).collect();
        if cells.len() < 9 {
            continue;
        }
        let ch = parse_chapter_num(cells[1]);
        if ch == 0 {
            continue;
        }
        entries.push(RelationEntry {
            chapter: format!("Ch{ch}"),
            speaker: cells[2].to_string(),
            object: cells[3].to_string(),
            relation: cells[5].to_string(),
            calling_speaker_to_object: cells[6].to_string(),
            calling_object_to_speaker: cells[7].to_string(),
            event: cells[8].to_string(),
            raw_row: line.to_string(),
        });
    }
    entries
}

fn find_character_relations(
    entries: &[RelationEntry],
    character: &str,
    target: Option<&str>,
) -> Vec<RelationEntry> {
    let a_lower = character.to_lowercase();
    let t_lower = target.map(|t| t.to_lowercase());
    entries
        .iter()
        .filter(|e| {
            // Match only speaker/object columns (cells[2]/[3]), not the event column (cells[8]).
            // Template: | 章节 | 说话者 | 对象 | 旧关系 | 新关系 | 说话者称呼变化 | 对方对说话者称呼 | 触发事件 |
            let raw = &e.raw_row;
            let cells: Vec<&str> = raw.split('|').map(|s| s.trim()).collect();
            let col_a = cells.get(2).map(|s| s.to_lowercase()).unwrap_or_default();
            let col_b = cells.get(3).map(|s| s.to_lowercase()).unwrap_or_default();
            let a_match = col_a.contains(&a_lower) || col_b.contains(&a_lower);
            if let Some(ref t) = t_lower {
                a_match && (col_a.contains(t) || col_b.contains(t))
            } else {
                a_match
            }
        })
        .cloned()
        .collect()
}

pub struct RelationQueryTool;

#[async_trait]
impl Tool for RelationQueryTool {
    fn name(&self) -> &str {
        "RelationQuery"
    }
    fn description(&self) -> &str {
        "Query character relationships and calling conventions from _关系与称呼索引.md"
    }
    fn input_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "character": {
                    "type": "string",
                    "description": "Character name to query relations for"
                },
                "target": {
                    "type": "string",
                    "description": "Optional: filter to relations with this specific character"
                },
                "include_history": {
                    "type": "boolean",
                    "description": "Include full relation change history (default false, current only)"
                }
            },
            "required": ["character"]
        })
    }
    fn is_read_only(&self) -> bool {
        true
    }

    async fn call(&self, input: Value, ctx: &ToolContext) -> Result<ToolOutput, ToolError> {
        let character = require_str(&input, "character")?;
        let target = input.get("target").and_then(|v| v.as_str());
        let include_history = input
            .get("include_history")
            .and_then(|v| v.as_bool())
            .unwrap_or(false);

        let store = KnowledgeStore::new(&ctx.project_root);
        let path = "knowledge/characters/_关系与称呼索引.md";
        let content = store.read_file(path).unwrap_or_default();

        let entries = parse_relation_table(&content);
        let matched = find_character_relations(&entries, &character, target);

        let current = if !matched.is_empty() {
            matched.last().cloned()
        } else {
            None
        };

        let history = if include_history && matched.len() > 1 {
            Some(matched[..matched.len() - 1].to_vec())
        } else {
            None
        };

        let result = RelationQueryResult {
            character: character.to_string(),
            target: target.map(|s| s.to_string()),
            current,
            history,
            all_relations: matched,
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

    fn write_relation_index(root: &std::path::Path, body: &str) {
        let dir = root.join("knowledge/characters");
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("_关系与称呼索引.md"), body).unwrap();
    }

    #[tokio::test(flavor = "current_thread")]
    async fn query_character_current_relation() {
        let tmp = TempDir::new().unwrap();
        write_relation_index(
            tmp.path(),
            "## 关系演变日志\n\
             | 章节 | 说话者 | 对象 | 旧关系 | 新关系 | 说话者称呼变化 | 对方对说话者称呼 | 触发事件 |\n\
             |------|--------|------|--------|--------|--------------|---------------|----------|\n\
             | Ch3 | 陈默 | 林若烟 | 陌生 | 陌生 | 陈前辈 | 丫头 | 初见 |\n\
             | Ch5 | 陈默 | 林若烟 | 陌生 | 亲近 | 陈默 | 若烟 | 救命之恩 |\n",
        );
        let tool = RelationQueryTool;
        let ctx = ToolContext {
            permission_mode: PermissionMode::Auto,
            project_root: tmp.path().to_path_buf(),
            ..ToolContext::new(tmp.path().to_path_buf())
        };
        let out = tool.call(json!({"character": "陈默"}), &ctx).await.unwrap();
        assert!(out.content.contains("陈默"));
        assert!(out.content.contains("林若烟"));
        assert!(out.content.contains("亲近"));
    }

    #[tokio::test(flavor = "current_thread")]
    async fn query_with_target_filter() {
        let tmp = TempDir::new().unwrap();
        write_relation_index(
            tmp.path(),
            "## 关系演变日志\n\
             | 章节 | 说话者 | 对象 | 旧关系 | 新关系 | 说话者称呼变化 | 对方对说话者称呼 | 触发事件 |\n\
             |------|--------|------|--------|--------|--------------|---------------|----------|\n\
             | Ch3 | 陈默 | 林若烟 | 陌生 | 陌生 | — | — | 初见 |\n\
             | Ch4 | 陈默 | 苏婉清 | 陌生 | 敌对 | — | — | 冲突 |\n\
             | Ch5 | 陈默 | 林若烟 | 陌生 | 亲近 | 陈默 | 若烟 | 救命 |\n",
        );
        let tool = RelationQueryTool;
        let ctx = ToolContext {
            permission_mode: PermissionMode::Auto,
            project_root: tmp.path().to_path_buf(),
            ..ToolContext::new(tmp.path().to_path_buf())
        };
        let out = tool
            .call(json!({"character": "陈默", "target": "林若烟"}), &ctx)
            .await
            .unwrap();
        assert!(out.content.contains("林若烟"));
        assert!(!out.content.contains("苏婉清"));
    }

    #[tokio::test(flavor = "current_thread")]
    async fn relation_query_excludes_third_party_in_event_column() {
        let tmp = TempDir::new().unwrap();
        write_relation_index(
            tmp.path(),
            "## 关系演变日志\n\
             | 章节 | 说话者 | 对象 | 旧关系 | 新关系 | 说话者称呼变化 | 对方对说话者称呼 | 触发事件 |\n\
             |------|--------|------|--------|--------|--------------|---------------|----------|\n\
             | Ch8 | 陈默 | 林若烟 | 亲近 | 亲近 | 陈默 | 若烟 | 陈默向苏婉清提起林若烟 |\n",
        );
        let tool = RelationQueryTool;
        let ctx = ToolContext {
            permission_mode: PermissionMode::Auto,
            project_root: tmp.path().to_path_buf(),
            ..ToolContext::new(tmp.path().to_path_buf())
        };
        // "苏婉清" 只出现在事件列——不应匹配
        let out = tool
            .call(json!({"character": "苏婉清"}), &ctx)
            .await
            .unwrap();
        assert!(out.content.contains("\"all_relations\": []"));
    }

    #[tokio::test(flavor = "current_thread")]
    async fn missing_file_returns_empty() {
        let tmp = TempDir::new().unwrap();
        let tool = RelationQueryTool;
        let ctx = ToolContext {
            permission_mode: PermissionMode::Auto,
            project_root: tmp.path().to_path_buf(),
            ..ToolContext::new(tmp.path().to_path_buf())
        };
        let out = tool
            .call(json!({"character": "不存在"}), &ctx)
            .await
            .unwrap();
        assert!(out.content.contains("\"all_relations\": []"));
    }
}
