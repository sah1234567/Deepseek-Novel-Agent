use super::common::{
    count_chinese_chars, default_outline_column_map, list_chapter_files, list_outline_files,
    parse_chapter_num, parse_outline_with_header,
};
use crate::{Tool, ToolContext, ToolError, ToolOutput};
use async_trait::async_trait;
use novel_knowledge::KnowledgeStore;
use serde::Serialize;
use serde_json::{json, Value};

#[derive(Debug, Serialize)]
struct StatsResult {
    total_words: u64,
    chapters_completed: u32,
    avg_words_per_chapter: f32,
    words_today: u64,
    words_this_week: u64,
    writing_streak_days: u32,
    estimated_completion: Option<f32>,
    per_chapter_word_distribution: Vec<(String, u32)>,
}

pub struct StatsTool;

fn parse_period(input: &Value) -> Option<&str> {
    input.get("period").and_then(|v| v.as_str())
}

async fn git_commit_dates(root: &std::path::Path) -> Vec<String> {
    let root = root.to_path_buf();
    crate::blocking::run_blocking(move || {
        let output = std::process::Command::new("git")
            .args([
                "log",
                "--since=30 days ago",
                "--format=%as",
                "--",
                "chapters/",
            ])
            .current_dir(&root)
            .output();
        match output {
            Ok(o) if o.status.success() => Ok(String::from_utf8_lossy(&o.stdout)
                .lines()
                .map(|s| s.trim().to_string())
                .filter(|s| !s.is_empty())
                .collect()),
            _ => Ok(vec![]),
        }
    })
    .await
    .unwrap_or_default()
}

fn count_words_in_period(distribution: &[(String, u32)], _period: Option<&str>) -> (u64, u64) {
    let total: u64 = distribution.iter().map(|(_, w)| *w as u64).sum();
    (total, total)
}

#[async_trait]
impl Tool for StatsTool {
    fn name(&self) -> &str {
        "Stats"
    }
    fn description(&self) -> &str {
        "Writing statistics from chapters and outlines"
    }
    fn input_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "period": {
                    "type": "string",
                    "enum": ["today", "this_week", "this_month", "all"]
                }
            }
        })
    }
    fn is_read_only(&self) -> bool {
        true
    }

    async fn call(&self, input: Value, ctx: &ToolContext) -> Result<ToolOutput, ToolError> {
        let period = parse_period(&input);
        let store = KnowledgeStore::new(&ctx.project_root);

        let mut distribution: Vec<(String, u32)> = Vec::new();

        for (num, path) in list_chapter_files(&ctx.project_root) {
            if let Ok(content) = crate::blocking::read_to_string(path).await {
                distribution.push((format!("Ch{num}"), count_chinese_chars(&content)));
            }
        }

        if distribution.is_empty() {
            for (num, path) in list_outline_files(&ctx.project_root) {
                if let Ok(content) = crate::blocking::read_to_string(path).await {
                    let words = content
                        .lines()
                        .find(|l| l.contains("总字数") || l.contains("实际字数"))
                        .and_then(|l| {
                            l.chars()
                                .filter(|c| c.is_ascii_digit())
                                .collect::<String>()
                                .parse::<u32>()
                                .ok()
                        })
                        .unwrap_or_else(|| count_chinese_chars(&content));
                    distribution.push((format!("Ch{num}"), words));
                }
            }
        }

        let total_words: u64 = distribution.iter().map(|(_, w)| *w as u64).sum();
        let chapters_completed = distribution.len() as u32;
        let avg = if chapters_completed > 0 {
            total_words as f32 / chapters_completed as f32
        } else {
            0.0
        };

        let (words_today, words_this_week) = count_words_in_period(&distribution, period);
        let git_dates = git_commit_dates(&ctx.project_root).await;
        let writing_streak_days = if git_dates.is_empty() {
            0
        } else {
            git_dates.len() as u32
        };

        let mut planned = 0u32;
        if let Ok(outline) = store.read_file("knowledge/plot/大纲.md") {
            let (colmap, _, data_rows) = parse_outline_with_header(&outline);
            let map: &super::common::OutlineColumnMap = match &colmap {
                Some(m) => m,
                None => default_outline_column_map(),
            };
            let chapter_idx = map.get("章").or_else(|| map.get("章节")).copied().unwrap_or(1);
            for cells in data_rows {
                if let Some(ch) = cells.get(chapter_idx) {
                    if parse_chapter_num(ch) > 0 {
                        planned += 1;
                    }
                }
            }
        }
        let estimated_completion = if planned > 0 {
            Some(chapters_completed as f32 / planned as f32 * 100.0)
        } else {
            None
        };

        let result = StatsResult {
            total_words,
            chapters_completed,
            avg_words_per_chapter: avg,
            words_today,
            words_this_week,
            writing_streak_days,
            estimated_completion,
            per_chapter_word_distribution: distribution,
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
    async fn stats_from_chapters() {
        let tmp = TempDir::new().unwrap();
        std::fs::create_dir_all(tmp.path().join("chapters")).unwrap();
        std::fs::write(
            tmp.path().join("chapters/chapter-001.md"),
            "林若烟入门测试，灵根异常。",
        )
        .unwrap();
        let tool = StatsTool;
        let ctx = ToolContext {
            permission_mode: PermissionMode::Auto,
            project_root: tmp.path().to_path_buf(),
            ..ToolContext::new(tmp.path().to_path_buf())
        };
        let out = tool.call(json!({"period": "all"}), &ctx).await.unwrap();
        assert!(out.content.contains("\"chapters_completed\": 1"));
        assert!(out.content.contains("\"total_words\""));
    }
}
