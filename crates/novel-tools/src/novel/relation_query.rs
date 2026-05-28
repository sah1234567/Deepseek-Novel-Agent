use super::common::parse_chapter_num;
use crate::{require_str, Tool, ToolContext, ToolError, ToolOutput};
use async_trait::async_trait;
use novel_knowledge::KnowledgeStore;
use serde::Serialize;
use serde_json::{json, Value};

#[derive(Debug, Clone, Serialize)]
struct RelationEntry {
    chapter: String,
    relation: String,
    calling_a_to_b: String,
    calling_b_to_a: String,
    event: String,
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
        if cells.len() < 7 {
            continue;
        }
        let ch = parse_chapter_num(cells[1]);
        if ch == 0 {
            continue;
        }
        entries.push(RelationEntry {
            chapter: format!("Ch{ch}"),
            relation: cells[3].to_string(),
            calling_a_to_b: cells[4].to_string(),
            calling_b_to_a: cells[5].to_string(),
            event: cells.get(6).map(|s| s.to_string()).unwrap_or_default(),
        });
    }
    entries
}

fn find_character_relations(
    entries: &[RelationEntry],
    a: &str,
    target: Option<&str>,
) -> Vec<RelationEntry> {
    let a_lower = a.to_lowercase();
    let t_lower = target.map(|t| t.to_lowercase());
    entries
        .iter()
        .filter(|e| {
            let row_text = format!(
                "{} {} {} {} {}",
                e.relation, e.calling_a_to_b, e.calling_b_to_a, e.event, e.chapter
            );
            let lower = row_text.to_lowercase();
            let has_a = lower.contains(&a_lower);
            if let Some(ref t) = t_lower {
                has_a && lower.contains(t)
            } else {
                has_a
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
             | 章节 | A | B | 关系 | A→B称呼 | B→A称呼 | 事件 |\n\
             |------|---|---|------|---------|---------|------|\n\
             | Ch3 | 陈默 | 林若烟 | 陌生 | —→\"陈前辈\" | —→\"丫头\" | 初见 |\n\
             | Ch5 | 陈默 | 林若烟 | 亲近 | \"陈前辈\"→\"陈默\" | \"丫头\"→\"若烟\" | 救命之恩 |\n",
        );
        let tool = RelationQueryTool;
        let ctx = ToolContext {
            permission_mode: PermissionMode::Auto,
            project_root: tmp.path().to_path_buf(),
            ..ToolContext::new(tmp.path().to_path_buf())
        };
        let out = tool
            .call(json!({"character": "陈默"}), &ctx)
            .await
            .unwrap();
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
             | 章节 | A | B | 关系 | A→B称呼 | B→A称呼 | 事件 |\n\
             |------|---|---|------|---------|---------|------|\n\
             | Ch3 | 陈默 | 林若烟 | 陌生 | — | — | 初见 |\n\
             | Ch4 | 陈默 | 苏婉清 | 敌对 | — | — | 冲突 |\n\
             | Ch5 | 陈默 | 林若烟 | 亲近 | 陈前辈→陈默 | 丫头→若烟 | 救命 |\n",
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
