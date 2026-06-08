use super::common::{count_chinese_chars, list_chapter_files};
use crate::{Tool, ToolContext, ToolError, ToolOutput, ValidationError};
use async_trait::async_trait;
use serde_json::{json, Value};

fn parse_chapter_param(input: &Value) -> Result<&str, ValidationError> {
    input
        .get("chapter")
        .and_then(|v| v.as_str())
        .filter(|s| !s.is_empty())
        .ok_or_else(|| ValidationError::MissingField("chapter".into()))
}

fn single_chapter_path(root: &std::path::Path, num: u32) -> Option<std::path::PathBuf> {
    let path = root.join("chapters").join(format!("chapter-{num:03}.md"));
    if path.exists() {
        Some(path)
    } else {
        None
    }
}

#[async_trait]
impl Tool for StatsTool {
    fn name(&self) -> &str {
        "Stats"
    }
    fn description(&self) -> &str {
        "Chapter word count. Pass a chapter number (e.g. \"31\") for a single chapter or \"all\" for aggregate stats across all chapters."
    }
    fn input_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "chapter": {
                    "type": "string",
                    "description": "Chapter number as string (e.g. \"31\" for chapter-031.md) or \"all\" for aggregate stats across all chapters."
                }
            },
            "required": ["chapter"]
        })
    }
    fn is_read_only(&self) -> bool {
        true
    }

    fn validate_input(&self, input: &Value) -> Result<(), ValidationError> {
        let chapter = parse_chapter_param(input)?;
        if chapter != "all" && chapter.parse::<u32>().is_err() {
            return Err(ValidationError::InvalidField(format!(
                "chapter must be a number or \"all\", got: {chapter}"
            )));
        }
        Ok(())
    }

    async fn call(&self, input: Value, ctx: &ToolContext) -> Result<ToolOutput, ToolError> {
        let chapter = parse_chapter_param(&input).map_err(ToolError::Validation)?;

        if chapter != "all" {
            let num: u32 = chapter.parse().map_err(|_| {
                ToolError::Validation(ValidationError::InvalidField(format!(
                    "invalid chapter number: {chapter}"
                )))
            })?;

            let path = single_chapter_path(&ctx.project_root, num).ok_or_else(|| {
                ToolError::Execution(format!(
                    "chapter file not found: chapters/chapter-{num:03}.md"
                ))
            })?;

            let content = crate::blocking::read_to_string(path)
                .await
                .map_err(|e| ToolError::Execution(format!("read chapter {num}: {e}")))?;

            let wc = count_chinese_chars(&content);
            return Ok(ToolOutput {
                content: format!("Ch{num}: {wc} 字"),
                is_error: false,
            });
        }

        // ── "all" mode ──
        let mut total_words: u64 = 0;
        let mut chapters_completed: u32 = 0;

        for (_num, path) in list_chapter_files(&ctx.project_root) {
            if let Ok(content) = crate::blocking::read_to_string(path).await {
                total_words += count_chinese_chars(&content) as u64;
                chapters_completed += 1;
            }
        }

        if chapters_completed == 0 {
            return Err(ToolError::Execution(
                "no chapter files found in chapters/".into(),
            ));
        }

        let avg = total_words as f32 / chapters_completed as f32;

        Ok(ToolOutput {
            content: format!(
                "总计 {total_words} 字, {chapters_completed} 章已完成, 平均 {avg:.0} 字/章"
            ),
            is_error: false,
        })
    }
}

pub struct StatsTool;

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{PermissionMode, ToolContext};
    use tempfile::TempDir;

    #[tokio::test(flavor = "current_thread")]
    async fn stats_single_chapter() {
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
        let out = tool.call(json!({"chapter": "1"}), &ctx).await.unwrap();
        assert!(out.content.contains("Ch1"));
        assert!(
            out.content.contains("字"),
            "should return plain text, not JSON"
        );
    }

    #[tokio::test(flavor = "current_thread")]
    async fn stats_all_chapters() {
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
        let out = tool.call(json!({"chapter": "all"}), &ctx).await.unwrap();
        assert!(out.content.contains("章已完成"));
        assert!(out.content.contains("总计"), "should return plain text");
    }

    #[tokio::test(flavor = "current_thread")]
    async fn stats_missing_chapter() {
        let tmp = TempDir::new().unwrap();
        std::fs::create_dir_all(tmp.path().join("chapters")).unwrap();
        let tool = StatsTool;
        let ctx = ToolContext {
            permission_mode: PermissionMode::Auto,
            project_root: tmp.path().to_path_buf(),
            ..ToolContext::new(tmp.path().to_path_buf())
        };
        let err = tool.call(json!({"chapter": "99"}), &ctx).await.unwrap_err();
        assert!(err.to_string().contains("not found"));
    }

    #[test]
    fn validate_rejects_empty_chapter() {
        let tool = StatsTool;
        assert!(tool.validate_input(&json!({"chapter": ""})).is_err());
        assert!(tool.validate_input(&json!({"chapter": "all"})).is_ok());
        assert!(tool.validate_input(&json!({"chapter": "31"})).is_ok());
    }
}
