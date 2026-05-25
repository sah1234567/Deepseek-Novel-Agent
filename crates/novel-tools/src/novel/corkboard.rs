use super::common::{count_chinese_chars, in_chapter_range, list_outline_files};
use crate::{Tool, ToolContext, ToolError, ToolOutput};
use async_trait::async_trait;
use regex::Regex;
use std::sync::OnceLock;
use serde::Serialize;
use serde_json::{json, Value};

#[derive(Debug, Serialize)]
struct CorkboardCard {
    chapter: String,
    scene_number: u32,
    scene_title: String,
    summary: String,
    pov: String,
    characters: Vec<String>,
    foreshadowings: Vec<String>,
    word_count_estimate: u32,
}

#[derive(Debug, Serialize)]
struct CorkboardResult {
    cards: Vec<CorkboardCard>,
}

pub struct CorkboardTool;

fn scene_regex() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| {
        Regex::new(r"(?m)^###\s*场景\s*(\d+)[:：]\s*(.+)$").expect("valid scene regex")
    })
}

fn word_count_regex() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| Regex::new(r"(\d+)\s*字").expect("valid word count regex"))
}

fn foreshadow_regex() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| Regex::new(r"\b(F\d+[a-z]?)\b").expect("valid foreshadow ref regex"))
}

fn parse_scene_sections(content: &str, chapter_num: u32) -> Vec<CorkboardCard> {
    let mut cards = Vec::new();
    let scene_re = scene_regex();
    let mut scene_starts: Vec<(u32, String, usize)> = Vec::new();
    for (idx, cap) in scene_re.captures_iter(content).enumerate() {
        let num = cap
            .get(1)
            .and_then(|m| m.as_str().parse().ok())
            .unwrap_or((idx + 1) as u32);
        let title = cap
            .get(2)
            .map(|m| m.as_str().trim())
            .unwrap_or("")
            .to_string();
        let line_idx = content[..cap.get(0).expect("capture group 0").start()].lines().count();
        scene_starts.push((num, title, line_idx));
    }

    let lines: Vec<&str> = content.lines().collect();
    let chapter = format!("Ch{chapter_num}");

    if scene_starts.is_empty() {
        let summary: String = content
            .lines()
            .find(|l| l.contains("章节摘要") || l.contains("本章目标"))
            .unwrap_or("")
            .chars()
            .take(300)
            .collect();
        cards.push(CorkboardCard {
            chapter: chapter.clone(),
            scene_number: 1,
            scene_title: content
                .lines()
                .find(|l| l.starts_with('#'))
                .unwrap_or("场景")
                .trim_start_matches('#')
                .trim()
                .to_string(),
            summary,
            pov: extract_pov(content),
            characters: extract_characters(content),
            foreshadowings: extract_foreshadowings(content),
            word_count_estimate: count_chinese_chars(content) / scene_starts.len().max(1) as u32,
        });
        return cards;
    }

    for (i, (scene_num, title, start_line)) in scene_starts.iter().enumerate() {
        let end_line = scene_starts
            .get(i + 1)
            .map(|(_, _, l)| *l)
            .unwrap_or(lines.len());
        let section: String = lines[*start_line..end_line].join("\n");
        let summary: String = section
            .lines()
            .filter(|l| !l.starts_with('#') && !l.starts_with('-'))
            .take(3)
            .collect::<Vec<_>>()
            .join(" ")
            .chars()
            .take(300)
            .collect();
        let word_est = section
            .lines()
            .find(|l| l.contains('字'))
            .and_then(|l| {
                word_count_regex().captures(l)
                    .and_then(|c| c.get(1))
                    .and_then(|m| m.as_str().parse().ok())
            })
            .unwrap_or(count_chinese_chars(&section));

        let section_pov = extract_pov(&section);
        cards.push(CorkboardCard {
            chapter: chapter.clone(),
            scene_number: *scene_num,
            scene_title: title.clone(),
            summary,
            pov: if section_pov == "—" {
                extract_pov(content)
            } else {
                section_pov
            },
            characters: extract_characters(&section),
            foreshadowings: extract_foreshadowings(&section),
            word_count_estimate: word_est,
        });
    }
    cards
}

fn extract_pov(text: &str) -> String {
    for line in text.lines() {
        if line.contains("POV") && line.contains('✓') {
            if let Some(name) = line.split('|').nth(1) {
                return name.trim().to_string();
            }
        }
        if line.contains("POV:") || line.contains("POV：") {
            return line
                .split([':', '：'])
                .nth(1)
                .unwrap_or("—")
                .trim()
                .to_string();
        }
    }
    "—".into()
}

fn extract_characters(text: &str) -> Vec<String> {
    let mut chars = Vec::new();
    for line in text.lines() {
        if line.contains("出场:") || line.contains("出场：") {
            let part = line.split([':', '：']).nth(1).unwrap_or("");
            for name in part.split([',', '，', '、']) {
                let n = name.trim().to_string();
                if !n.is_empty() && !chars.contains(&n) {
                    chars.push(n);
                }
            }
        }
    }
    chars
}

fn extract_foreshadowings(text: &str) -> Vec<String> {
    let mut fs = Vec::new();
    let re = foreshadow_regex();
    for cap in re.captures_iter(text) {
        if let Some(id) = cap.get(1) {
            let s = id.as_str().to_string();
            if !fs.contains(&s) {
                fs.push(s);
            }
        }
    }
    fs
}

#[async_trait]
impl Tool for CorkboardTool {
    fn name(&self) -> &str {
        "Corkboard"
    }
    fn description(&self) -> &str {
        "Scene cards from detailed chapter outlines"
    }
    fn input_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "chapter_range": {
                    "type": "array",
                    "items": {"type": "integer"},
                    "minItems": 2,
                    "maxItems": 2
                },
                "filter": {
                    "type": "object",
                    "properties": {
                        "pov": {"type": "string"},
                        "character": {"type": "string"},
                        "foreshadowing": {"type": "string"}
                    }
                }
            }
        })
    }
    fn is_read_only(&self) -> bool {
        true
    }

    async fn call(&self, input: Value, ctx: &ToolContext) -> Result<ToolOutput, ToolError> {
        let range = input
            .get("chapter_range")
            .and_then(|v| v.as_array())
            .and_then(|a| {
                if a.len() == 2 {
                    Some((a[0].as_u64()? as u32, a[1].as_u64()? as u32))
                } else {
                    None
                }
            });
        let filter_pov = input
            .get("filter")
            .and_then(|f| f.get("pov"))
            .and_then(|v| v.as_str());
        let filter_char = input
            .get("filter")
            .and_then(|f| f.get("character"))
            .and_then(|v| v.as_str());
        let filter_fs = input
            .get("filter")
            .and_then(|f| f.get("foreshadowing"))
            .and_then(|v| v.as_str());

        let mut cards = Vec::new();
        for (num, path) in list_outline_files(&ctx.project_root) {
            if !in_chapter_range(num, range) {
                continue;
            }
            if let Ok(content) = std::fs::read_to_string(path) {
                cards.extend(parse_scene_sections(&content, num));
            }
        }

        if let Some(pov) = filter_pov {
            cards.retain(|c| c.pov.contains(pov));
        }
        if let Some(ch) = filter_char {
            cards.retain(|c| c.characters.iter().any(|x| x.contains(ch)));
        }
        if let Some(fs) = filter_fs {
            cards.retain(|c| c.foreshadowings.iter().any(|x| x == fs));
        }

        let result = CorkboardResult { cards };
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
    async fn corkboard_no_outlines() {
        let tmp = TempDir::new().unwrap();
        let tool = CorkboardTool;
        let ctx = ToolContext {
            permission_mode: PermissionMode::Auto,
            project_root: tmp.path().to_path_buf(),
            ..ToolContext::new(tmp.path().to_path_buf())
        };
        let out = tool.call(json!({}), &ctx).await.unwrap();
        assert!(out.content.contains("\"cards\": []"));
    }

    #[tokio::test(flavor = "current_thread")]
    async fn corkboard_parses_scenes() {
        let tmp = TempDir::new().unwrap();
        let dir = tmp.path().join("knowledge/plot/细纲");
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(
            dir.join("chapter-031-细纲.md"),
            "# Chapter 31\n### 场景 1: 矿洞入口（~350字）\n- 林若烟追踪陈默\n- 出场: 林若烟\n- F04推进\n",
        )
        .unwrap();
        let tool = CorkboardTool;
        let ctx = ToolContext {
            permission_mode: PermissionMode::Auto,
            project_root: tmp.path().to_path_buf(),
            ..ToolContext::new(tmp.path().to_path_buf())
        };
        let out = tool
            .call(json!({"chapter_range": [31, 31]}), &ctx)
            .await
            .unwrap();
        assert!(out.content.contains("矿洞入口"));
        assert!(out.content.contains("林若烟"));
    }
}
