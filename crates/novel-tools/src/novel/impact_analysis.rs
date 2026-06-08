use super::common::{list_character_names, parse_chapter_num};
use crate::{require_str, Tool, ToolContext, ToolError, ToolOutput};
use async_trait::async_trait;
use novel_knowledge::{find_table_last_row, KnowledgeStore};
use serde::Serialize;
use serde_json::{json, Value};

#[derive(Debug, Serialize)]
pub(crate) struct ImpactFile {
    path: String,
    reason: String,
}

#[derive(Debug, Serialize)]
struct ImpactAnalysisResult {
    target: String,
    affected_files: Vec<ImpactFile>,
    foreshadow_ids: Vec<String>,
    causality_events: Vec<String>,
    requires_confirmation: bool,
}

pub struct ImpactAnalysisTool;

fn collect_appearance_impacts(
    store: &KnowledgeStore,
    target_ch: u32,
    names: &[String],
) -> Vec<ImpactFile> {
    let mut out = Vec::new();
    for name in names {
        let rel = format!("knowledge/characters/{name}.md");
        let Ok(content) = store.read_file(&rel) else {
            continue;
        };
        // Scope to 出场记录日志 table only — same fix as CharacterRotate.
        let section = content
            .find("## 出场记录日志")
            .map(|i| &content[i..])
            .unwrap_or(&content);
        let section_end = section.find("\n## ").unwrap_or(section.len());
        for line in section[..section_end].lines() {
            if !line.starts_with('|') || line.contains("章节") || line.contains("---") {
                continue;
            }
            if parse_chapter_num(line) >= target_ch {
                out.push(ImpactFile {
                    path: rel.clone(),
                    reason: format!("出场记录 Ch{target_ch} 及之后"),
                });
                break;
            }
        }
    }
    out
}

fn collect_foreshadow_impacts(
    store: &KnowledgeStore,
    target_ch: u32,
) -> (Vec<ImpactFile>, Vec<String>) {
    let path = "knowledge/plot/伏笔追踪.md";
    let Ok(content) = store.read_file(path) else {
        return (vec![], vec![]);
    };
    let mut files = Vec::new();
    let mut ids = Vec::new();
    for line in content.lines() {
        if !line.starts_with('|') || line.contains("章节") {
            continue;
        }
        let cells: Vec<&str> = line.split('|').map(|s| s.trim()).collect();
        if cells.len() < 7 {
            continue;
        }
        let ch = parse_chapter_num(cells[1]);
        if ch >= target_ch {
            ids.push(cells[2].to_string());
            files.push(ImpactFile {
                path: path.into(),
                reason: format!("伏笔 {} 在 Ch{ch} 埋设/更新", cells[2]),
            });
        }
    }
    (files, ids)
}

pub(crate) fn collect_causality_impacts(
    store: &KnowledgeStore,
    target_ch: u32,
) -> (Vec<ImpactFile>, Vec<String>) {
    let path = "knowledge/plot/因果链.md";
    let Ok(content) = store.read_file(path) else {
        return (vec![], vec![]);
    };
    let mut files = Vec::new();
    let mut events = Vec::new();
    for line in content.lines() {
        if !line.starts_with('|') || line.contains("---") || line.contains("章节") {
            continue;
        }
        let cols: Vec<&str> = line
            .split('|')
            .map(|s| s.trim())
            .filter(|s| !s.is_empty())
            .collect();
        if cols.len() < 2 {
            continue;
        }
        let ch = parse_chapter_num(cols[0]);
        let id = cols[1];
        if ch >= target_ch && !id.is_empty() && id != "—" {
            events.push(id.to_string());
        }
    }
    if !events.is_empty() {
        files.push(ImpactFile {
            path: path.into(),
            reason: "因果链可能需截断或标记废弃".into(),
        });
    }
    (files, events)
}

#[async_trait]
impl Tool for ImpactAnalysisTool {
    fn name(&self) -> &str {
        "ImpactAnalysis"
    }
    fn description(&self) -> &str {
        "Analyze cascade impact of deleting or revising a chapter"
    }
    fn input_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "chapter_path": {"type": "string", "description": "Chapter file to delete or revise, e.g. chapters/chapter-031.md"},
                "action": {"type": "string", "enum": ["delete", "revise"], "default": "delete"}
            },
            "required": ["chapter_path"]
        })
    }
    fn is_read_only(&self) -> bool {
        true
    }

    async fn call(&self, input: Value, ctx: &ToolContext) -> Result<ToolOutput, ToolError> {
        let chapter_path = require_str(&input, "chapter_path")?;
        let action = input
            .get("action")
            .and_then(|v| v.as_str())
            .unwrap_or("delete");
        let target_ch = parse_chapter_num(&chapter_path);
        if target_ch == 0 {
            return Err(ToolError::Execution(
                "cannot parse chapter number from chapter_path".into(),
            ));
        }

        let store = KnowledgeStore::new(&ctx.project_root);
        let names = list_character_names(&store);
        let mut affected = Vec::new();

        affected.extend(collect_appearance_impacts(&store, target_ch, &names));

        let (fs_files, foreshadow_ids) = collect_foreshadow_impacts(&store, target_ch);
        affected.extend(fs_files);

        let (causality_files, causality_events) = collect_causality_impacts(&store, target_ch);
        affected.extend(causality_files);

        for rel in [
            format!("knowledge/plot/细纲/chapter-{target_ch:03}-细纲.md"),
            chapter_path.clone(),
            "knowledge/plot/大纲.md".into(),
            "knowledge/INDEX.md".into(),
        ] {
            if store.root.join(&rel).exists() {
                affected.push(ImpactFile {
                    path: rel,
                    reason: format!("{action} 流程需更新或移除"),
                });
            }
        }

        if let Ok(content) = store.read_file("knowledge/shared-systems/时间线.md") {
            if let Ok(Some(row)) = find_table_last_row(&content, "时间线演变日志") {
                if parse_chapter_num(&row) >= target_ch {
                    affected.push(ImpactFile {
                        path: "knowledge/shared-systems/时间线.md".into(),
                        reason: "时间线演变日志含目标章及之后记录".into(),
                    });
                }
            }
        }

        affected.sort_by(|a, b| a.path.cmp(&b.path));
        affected.dedup_by(|a, b| a.path == b.path);

        let result = ImpactAnalysisResult {
            target: format!("Ch{target_ch} ({chapter_path})"),
            affected_files: affected,
            foreshadow_ids,
            causality_events,
            requires_confirmation: false,
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

    #[tokio::test(flavor = "current_thread")]
    async fn delete_chapter_lists_impacts() {
        let tmp = TempDir::new().unwrap();
        std::fs::create_dir_all(tmp.path().join("knowledge/characters")).unwrap();
        std::fs::create_dir_all(tmp.path().join("chapters")).unwrap();
        std::fs::write(
            tmp.path().join("knowledge/characters/主角.md"),
            "## 出场记录日志\n| 章节 | 事件 |\n|------|------|\n| Ch10 | 战斗 |\n",
        )
        .unwrap();
        std::fs::write(tmp.path().join("chapters/chapter-010.md"), "正文").unwrap();
        let tool = ImpactAnalysisTool;
        let ctx = ToolContext {
            permission_mode: PermissionMode::Auto,
            project_root: tmp.path().to_path_buf(),
            ..ToolContext::new(tmp.path().to_path_buf())
        };
        let out = tool
            .call(
                json!({"chapter_path": "chapters/chapter-010.md", "action": "delete"}),
                &ctx,
            )
            .await
            .unwrap();
        assert!(out.content.contains("affected_files"));
        assert!(out.content.contains("主角"));
    }

    #[test]
    fn collect_causality_impacts_finds_events_from_chapter() {
        let tmp = TempDir::new().unwrap();
        std::fs::create_dir_all(tmp.path().join("knowledge/plot")).unwrap();
        std::fs::write(
            tmp.path().join("knowledge/plot/因果链.md"),
            "| 章节 | 事件ID | 描述 |\n|------|--------|------|\n| Ch10 | E01 | 伏笔引爆 |\n",
        )
        .unwrap();
        let store = KnowledgeStore::new(tmp.path());
        let (files, events) = collect_causality_impacts(&store, 10);
        assert_eq!(events, vec!["E01".to_string()]);
        assert_eq!(files.len(), 1);
    }
}
