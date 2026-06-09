use super::super::{
    add_line_numbers, extract_file_path, file_mtime_secs, ReadCacheEntry, ReadCacheSource, Tool,
    ToolContext, ToolError, ToolOutput, ValidationError, FILE_UNCHANGED_STUB,
};
use async_trait::async_trait;
use serde_json::{json, Value};
use tokio::fs;

pub struct TailTool;

const DEFAULT_LINES: usize = 80;

#[async_trait]
impl Tool for TailTool {
    fn name(&self) -> &str {
        "Tail"
    }

    fn description(&self) -> &str {
        "Read the last N lines of a file. Use for continuity when continuing a chapter \
         (Tail chapters/chapter-{N-1}.md). Output uses {line}\\t{content} like Read. \
         For evolution log rows inside multi-table character cards, use Grep then Read range — \
         do not Tail the whole character file."
    }

    fn usage_hint(&self) -> &str {
        "Continuity: Tail previous chapter 80–120 lines. Same path+lines+mtime repeat returns stub. Character cards: Grep → Read range."
    }

    fn input_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "file_path": {
                    "type": "string",
                    "description": "Relative path under project root (e.g. chapters/chapter-019.md)"
                },
                "lines": {
                    "type": "integer",
                    "description": "Lines from end of file (default 80). Max 80 knowledge/memory/plan; max 200 chapters/."
                }
            },
            "required": ["file_path"]
        })
    }

    fn is_read_only(&self) -> bool {
        true
    }

    fn extract_read_span(&self, input: &Value, total_lines: usize) -> Option<(usize, usize)> {
        let lines_n = input.get("lines").and_then(|v| v.as_u64()).unwrap_or(80) as usize;
        let total = total_lines;
        let take = lines_n.min(total);
        let start = if take == 0 {
            1
        } else {
            total.saturating_sub(take) + 1
        };
        Some((start, total.max(start)))
    }

    fn supports_read_dedup_hint(&self) -> bool {
        true
    }

    fn read_dedup_range_label(&self, input: &Value) -> Option<String> {
        let _ = extract_file_path(input).ok()?;
        let tail_lines = input.get("lines").and_then(|v| v.as_u64()).unwrap_or(80) as usize;
        Some(format!("tail:last_{tail_lines}_lines"))
    }

    fn max_output_lines(&self, input: &Value) -> Option<usize> {
        let fp = extract_file_path(input).ok()?;
        Some(
            crate::read_economy::max_lines_for_path(&fp)
                .unwrap_or(crate::read_economy::CHAPTER_MAX_LINES),
        )
    }

    fn validate_input(&self, input: &Value) -> Result<(), ValidationError> {
        extract_file_path(input)?;
        Ok(())
    }

    async fn call(&self, input: Value, ctx: &ToolContext) -> Result<ToolOutput, ToolError> {
        let path = extract_file_path(&input)?;
        let full = ctx.resolve_path(&path);
        let lines_n = input
            .get("lines")
            .and_then(|v| v.as_u64())
            .unwrap_or(DEFAULT_LINES as u64) as usize;

        if let Some(max) = crate::read_economy::max_lines_for_path(&path) {
            if lines_n > max {
                return Err(ToolError::Execution(format!(
                    "Tail economy: lines {lines_n} exceeds max {max} for this path kind."
                )));
            }
        }

        ctx.with_file_lock(&full, || async {
            let metadata = fs::metadata(&full).await.map_err(ToolError::Io)?;
            if metadata.len() == 0 {
                let mtime = file_mtime_secs(&metadata);
                ctx.store_read_cache(
                    &full,
                    ReadCacheEntry {
                        mtime_secs: mtime,
                        raw_content: String::new(),
                        offset: None,
                        limit: None,
                        total_lines: 0,
                        source: ReadCacheSource::Tail,
                        transcript_committed: false,
                        committed_spans: Vec::new(),
                        committed_offset: None,
                        committed_limit: None,
                    },
                    None,
                    None,
                )?;
                return Ok(ToolOutput {
                    content: "<system-reminder>Warning: the file exists but the contents are empty.</system-reminder>"
                        .into(),
                    is_error: false,
                });
            }

            let mtime = file_mtime_secs(&metadata);
            let content = fs::read_to_string(&full).await.map_err(ToolError::Io)?;
            let all_lines: Vec<&str> = content.lines().collect();
            let total_lines = all_lines.len();
            let take = lines_n.min(total_lines);
            let start_idx = total_lines.saturating_sub(take);
            let start_line = if take == 0 { 1 } else { start_idx + 1 };

            if ctx.tail_dedup_hit(&full, start_line, take, total_lines, mtime) {
                return Ok(ToolOutput {
                    content: FILE_UNCHANGED_STUB.into(),
                    is_error: false,
                });
            }

            let slice: Vec<&str> = all_lines[start_idx..].to_vec();
            let raw = slice.join("\n");

            crate::read_economy::read_pre_check(&path, Some(lines_n), total_lines)?;

            let formatted = add_line_numbers(&raw, start_line);

            ctx.store_read_cache(
                &full,
                ReadCacheEntry {
                    mtime_secs: mtime,
                    raw_content: raw,
                    offset: Some(start_line),
                    limit: Some(take),
                    total_lines,
                    source: ReadCacheSource::Tail,
                    transcript_committed: false,
                    committed_spans: Vec::new(),
                    committed_offset: None,
                    committed_limit: None,
                },
                Some(&content),
                None,
            )?;

            Ok(ToolOutput {
                content: formatted,
                is_error: false,
            })
        })
        .await
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::PermissionMode;
    use std::io::Write;
    use std::sync::Arc;
    use tempfile::TempDir;

    fn test_ctx(tmp: &TempDir) -> ToolContext {
        let mut ctx = ToolContext::new(tmp.path().to_path_buf());
        ctx.read_file_cache = Some(Arc::new(dashmap::DashMap::new()));
        ctx.permission_mode = PermissionMode::Normal;
        ctx
    }

    fn write_file(dir: &std::path::Path, name: &str, content: &str) {
        let path = dir.join(name);
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).unwrap();
        }
        let mut f = std::fs::File::create(&path).unwrap();
        f.write_all(content.as_bytes()).unwrap();
    }

    #[tokio::test(flavor = "current_thread")]
    async fn tail_last_two_lines() {
        let tmp = TempDir::new().unwrap();
        write_file(tmp.path(), "chapters/chapter-001.md", "a\nb\nc\nd\ne");
        let ctx = test_ctx(&tmp);
        let out = TailTool
            .call(
                json!({"file_path": "chapters/chapter-001.md", "lines": 2}),
                &ctx,
            )
            .await
            .unwrap();
        assert!(out.content.contains("4\td"));
        assert!(out.content.contains("5\te"));
        assert!(!out.content.contains("3\tc"));
    }

    #[tokio::test(flavor = "current_thread")]
    async fn tail_updates_read_cache() {
        let tmp = TempDir::new().unwrap();
        write_file(tmp.path(), "test.md", "only line");
        let ctx = test_ctx(&tmp);
        TailTool
            .call(json!({"file_path": "test.md"}), &ctx)
            .await
            .unwrap();
        assert!(ctx.was_read(&ctx.resolve_path("test.md")));
    }
}
