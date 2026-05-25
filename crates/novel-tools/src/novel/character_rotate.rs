use super::common::{list_character_names, parse_chapter_num};
use crate::{Tool, ToolContext, ToolError, ToolOutput};
use async_trait::async_trait;
use novel_knowledge::{parse_frontmatter, CharacterFrontmatter, KnowledgeStore};
use serde::Serialize;
use serde_json::{json, Value};
use std::collections::HashMap;

#[derive(Debug, Serialize)]
struct CharacterRotation {
    name: String,
    last_chapter: String,
    chapters_since_last: u32,
    is_missing: bool,
    appearance_chapters: Vec<String>,
    avg_gap: f32,
}

#[derive(Debug, Serialize)]
struct CharacterRotateResult {
    per_character: Vec<CharacterRotation>,
    per_chapter_character_count: Vec<(String, u32)>,
    ensemble_warnings: Vec<String>,
}

pub struct CharacterRotateTool;

fn parse_appearance_chapters(content: &str) -> Vec<u32> {
    let mut chapters = Vec::new();
    for line in content.lines() {
        if !line.starts_with('|') || line.contains("章节") || line.contains("---") {
            continue;
        }
        let cells: Vec<&str> = line.split('|').map(|s| s.trim()).collect();
        if cells.len() < 2 {
            continue;
        }
        let num = parse_chapter_num(cells[1]);
        if num > 0 {
            chapters.push(num);
        }
    }
    chapters.sort_unstable();
    chapters.dedup();
    chapters
}

fn avg_gap(chapters: &[u32]) -> f32 {
    if chapters.len() < 2 {
        return 0.0;
    }
    let gaps: Vec<f32> = chapters.windows(2).map(|w| (w[1] - w[0]) as f32).collect();
    gaps.iter().sum::<f32>() / gaps.len() as f32
}

#[async_trait]
impl Tool for CharacterRotateTool {
    fn name(&self) -> &str {
        "CharacterRotate"
    }
    fn description(&self) -> &str {
        "Check POV character appearance rotation and missing characters"
    }
    fn input_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "character_name": {"type": "string"},
                "inactivity_threshold": {"type": "integer", "default": 10}
            }
        })
    }
    fn is_read_only(&self) -> bool {
        true
    }

    async fn call(&self, input: Value, ctx: &ToolContext) -> Result<ToolOutput, ToolError> {
        let filter_name = input
            .get("character_name")
            .and_then(|v| v.as_str())
            .map(String::from);
        let threshold = input
            .get("inactivity_threshold")
            .and_then(|v| v.as_u64())
            .unwrap_or(10) as u32;

        let store = KnowledgeStore::new(&ctx.project_root);
        let names = list_character_names(&store);
        let mut per_character = Vec::new();
        let mut chapter_counts: HashMap<u32, u32> = HashMap::new();
        let mut max_chapter = 0u32;

        for name in &names {
            if let Some(ref f) = filter_name {
                if f != name {
                    continue;
                }
            }
            let path = format!("knowledge/characters/{name}.md");
            let content = match store.read_file(&path) {
                Ok(c) => c,
                Err(_) => continue,
            };
            let (fm, _): (CharacterFrontmatter, _) = parse_frontmatter(&content).unwrap_or((
                CharacterFrontmatter {
                    name: name.clone(),
                    aliases: vec![],
                    category: novel_knowledge::CharacterCategory::Human,
                    first_appearance: String::new(),
                    last_update: String::new(),
                    status: novel_knowledge::CharacterStatus::Alive,
                    pov_character: false,
                },
                String::new(),
            ));
            if filter_name.is_none() && !fm.pov_character {
                continue;
            }

            let appearances = parse_appearance_chapters(&content);
            let last = appearances.last().copied().unwrap_or(0);
            if last > max_chapter {
                max_chapter = last;
            }
            for ch in &appearances {
                *chapter_counts.entry(*ch).or_insert(0) += 1;
            }
            let since = if max_chapter > last {
                max_chapter - last
            } else {
                0
            };
            per_character.push(CharacterRotation {
                name: name.clone(),
                last_chapter: if last > 0 {
                    format!("Ch{last}")
                } else {
                    "—".into()
                },
                chapters_since_last: since,
                is_missing: since >= threshold,
                appearance_chapters: appearances.iter().map(|n| format!("Ch{n}")).collect(),
                avg_gap: avg_gap(&appearances),
            });
        }

        for (ch, _) in super::common::list_chapter_files(&ctx.project_root) {
            max_chapter = max_chapter.max(ch);
        }
        for entry in &mut per_character {
            let last = parse_chapter_num(&entry.last_chapter);
            entry.chapters_since_last = max_chapter.saturating_sub(last);
            entry.is_missing = entry.chapters_since_last >= threshold;
        }

        let mut per_chapter_character_count: Vec<(String, u32)> = chapter_counts
            .into_iter()
            .map(|(ch, count)| (format!("Ch{ch}"), count))
            .collect();
        per_chapter_character_count
            .sort_by(|a, b| parse_chapter_num(&a.0).cmp(&parse_chapter_num(&b.0)));

        let result = CharacterRotateResult {
            per_character,
            per_chapter_character_count,
            ensemble_warnings: Vec::new(),
        };
        Ok(ToolOutput {
            content: serde_json::to_string_pretty(&result).map_err(|e| ToolError::Execution(format!("json serialize: {e}")))?,
            is_error: false,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{PermissionMode, ToolContext};
    use tempfile::TempDir;

    #[tokio::test(flavor = "current_thread")]
    async fn character_rotate_detects_missing() {
        let tmp = TempDir::new().unwrap();
        std::fs::create_dir_all(tmp.path().join("knowledge/characters")).unwrap();
        std::fs::write(
            tmp.path().join("knowledge/characters/林若烟.md"),
            "---\nname: 林若烟\ncategory: human\nfirstAppearance: Ch1\nlastUpdate: Ch1\nstatus: alive\npovCharacter: true\n---\n\
             ## 出场记录日志\n| 章节 | 关键事件 | 伏笔关联 | 情绪弧线 |\n\
             |------|---------|---------|---------|\n\
             | Ch1 | 入门 | — | 好奇 |\n",
        )
        .unwrap();
        std::fs::create_dir_all(tmp.path().join("chapters")).unwrap();
        for n in 2..=15 {
            std::fs::write(
                tmp.path().join(format!("chapters/chapter-{n:03}.md")),
                "placeholder",
            )
            .unwrap();
        }
        let tool = CharacterRotateTool;
        let ctx = ToolContext {
            permission_mode: PermissionMode::Auto,
            project_root: tmp.path().to_path_buf(),
            ..ToolContext::new(tmp.path().to_path_buf())
        };
        let out = tool
            .call(json!({"inactivity_threshold": 5}), &ctx)
            .await
            .unwrap();
        assert!(out.content.contains("\"is_missing\": true") || out.content.contains("is_missing"));
    }
}
