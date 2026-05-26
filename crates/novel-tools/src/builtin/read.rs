use super::super::{
    add_line_numbers, extract_file_path, file_mtime_secs, read_range_key, ReadCacheEntry,
    ReadCacheSource, Tool, ToolContext, ToolError, ToolOutput, ValidationError, FILE_UNCHANGED_STUB,
};
use async_trait::async_trait;
use serde_json::{json, Value};
use tokio::io::{AsyncBufReadExt, BufReader};
use tokio::fs;

pub struct ReadTool;

const MAX_OUTPUT_BYTES: usize = 256 * 1024; // 256 KB
const FAST_PATH_MAX_SIZE: u64 = 10 * 1024 * 1024; // 10 MB

fn format_file_size(bytes: usize) -> String {
    if bytes < 1024 {
        format!("{} B", bytes)
    } else if bytes < 1024 * 1024 {
        format!("{:.1} KB", bytes as f64 / 1024.0)
    } else {
        format!("{:.1} MB", bytes as f64 / (1024.0 * 1024.0))
    }
}

/// Fast path: read entire file into memory, slice by lines.
async fn read_fast(
    full: &std::path::Path,
    line_offset: usize,
    limit: Option<usize>,
) -> Result<(String, usize, usize), ToolError> {
    let content = fs::read_to_string(full)
        .await
        .map_err(|e| ToolError::Io(e))?;
    let all_lines: Vec<&str> = content.lines().collect();
    let total_lines = all_lines.len();

    if line_offset >= total_lines {
        return Ok((String::new(), 0, total_lines));
    }

    let end = match limit {
        Some(lim) => (line_offset + lim).min(total_lines),
        None => total_lines,
    };

    let selected: Vec<&str> = all_lines[line_offset..end].to_vec();
    let line_count = selected.len();
    let result = selected.join("\n");
    Ok((result, line_count, total_lines))
}

/// Streaming path: for files >= 10 MB, read line by line without loading entirely into memory.
async fn read_streaming(
    full: &std::path::Path,
    line_offset: usize,
    limit: Option<usize>,
) -> Result<(String, usize, usize), ToolError> {
    let file = fs::File::open(full).await.map_err(|e| ToolError::Io(e))?;
    let reader = BufReader::new(file);

    let end_line = match limit {
        Some(lim) => line_offset + lim,
        None => usize::MAX,
    };

    let mut selected = Vec::new();
    let mut total_lines = 0usize;
    let mut line_count = 0usize;
    let mut lines = reader.lines();

    loop {
        match lines.next_line().await {
            Ok(Some(line)) => {
                if total_lines >= line_offset && total_lines < end_line {
                    selected.push(line);
                    line_count += 1;
                }
                total_lines += 1;
                if total_lines >= end_line && limit.is_none() {
                    continue;
                }
            }
            Ok(None) => break,
            Err(e) => return Err(ToolError::Io(e)),
        }
    }

    let result = selected.join("\n");
    Ok((result, line_count, total_lines))
}

#[async_trait]
impl Tool for ReadTool {
    fn name(&self) -> &str {
        "Read"
    }
    fn description(&self) -> &str {
        "Read a file from the project with optional offset/limit for line-range reading. \
         Prefer Grep to locate content first, then Read only the needed lines. \
         Full-file reads are limited to 256 KB; use offset+limit or Grep for larger files."
    }
    fn usage_hint(&self) -> &str {
        "knowledge/** must use offset+limit (max 80 lines). Same path+range+mtime repeat returns stub — use earlier tool_result."
    }
    fn input_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "file_path": {
                    "type": "string",
                    "description": "Relative path under project root"
                },
                "offset": {
                    "type": "integer",
                    "description": "1-indexed start line (default 1)"
                },
                "limit": {
                    "type": "integer",
                    "description": "Max lines to read from offset"
                }
            },
            "required": ["file_path"]
        })
    }
    fn is_read_only(&self) -> bool {
        true
    }

    fn validate_input(&self, input: &Value) -> Result<(), ValidationError> {
        extract_file_path(input)?;
        Ok(())
    }

    async fn call(&self, input: Value, ctx: &ToolContext) -> Result<ToolOutput, ToolError> {
        let path = extract_file_path(&input)?;
        let full = ctx.resolve_path(&path);
        let offset = input.get("offset").and_then(|v| v.as_u64()).unwrap_or(1) as usize;
        let limit = input.get("limit").and_then(|v| v.as_u64()).map(|l| l as usize);

        // Offset is 1-indexed; convert to 0-indexed
        let line_offset = if offset == 0 { 0 } else { offset.saturating_sub(1) };

        // ── Empty file check ──
        match fs::metadata(&full).await {
            Ok(meta) if meta.len() == 0 => {
                ctx.store_read_cache(
                    &full,
                    ReadCacheEntry {
                        mtime_secs: file_mtime_secs(&meta),
                        raw_content: String::new(),
                        offset: None,
                        limit: None,
                        total_lines: 0,
                        source: ReadCacheSource::Read,
                    },
                );
                return Ok(ToolOutput {
                    content: "<system-reminder>Warning: the file exists but the contents are empty.</system-reminder>".into(),
                    is_error: false,
                });
            }
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                return Err(ToolError::Io(e));
            }
            _ => {}
        }

        // ── Dedup check (mtime + range) ──
        if let Ok(meta) = fs::metadata(&full).await {
            let current_mtime = file_mtime_secs(&meta);
            let (cache_off, cache_lim) = read_range_key(
                if limit.is_some() || offset > 1 {
                    Some(offset)
                } else {
                    None
                },
                limit,
            );
            if ctx.read_dedup_hit(&full, cache_off, cache_lim, current_mtime) {
                return Ok(ToolOutput {
                    content: FILE_UNCHANGED_STUB.into(),
                    is_error: false,
                });
            }
        }

        let metadata = fs::metadata(&full).await.map_err(|e| ToolError::Io(e))?;

        // ── Choose fast vs streaming path ──
        let (content, _line_count, total_lines) = if metadata.len() < FAST_PATH_MAX_SIZE {
            read_fast(&full, line_offset, limit).await?
        } else {
            read_streaming(&full, line_offset, limit).await?
        };

        crate::read_economy::read_pre_check(&path, limit, total_lines)?;

        // ── Offset beyond file ──
        if content.is_empty() && line_offset >= total_lines && total_lines > 0 {
            return Ok(ToolOutput {
                content: format!(
                    "<system-reminder>Warning: the file has only {} lines, but offset is {}. No content to read.</system-reminder>",
                    total_lines, offset
                ),
                is_error: false,
            });
        }

        // ── Byte limit check (only for full-file reads, not line-range) ──
        if limit.is_none() && content.len() > MAX_OUTPUT_BYTES {
            let size_str = format_file_size(content.len());
            let limit_str = format_file_size(MAX_OUTPUT_BYTES);
            return Err(ToolError::Execution(format!(
                "File content ({size_str}) exceeds maximum allowed size ({limit_str}). Use offset and limit parameters to read specific portions of the file, or use Grep to search for specific content."
            )));
        }

        // ── Add line numbers ──
        let formatted = if content.is_empty() {
            String::new()
        } else {
            add_line_numbers(&content, offset.max(1))
        };

        let mtime = file_mtime_secs(&metadata);
        let (cache_off, cache_lim) = read_range_key(
            if limit.is_some() || offset > 1 {
                Some(offset)
            } else {
                None
            },
            limit,
        );
        ctx.store_read_cache(
            &full,
            ReadCacheEntry {
                mtime_secs: mtime,
                raw_content: content,
                offset: cache_off,
                limit: cache_lim,
                total_lines,
                source: ReadCacheSource::Read,
            },
        );

        Ok(ToolOutput {
            content: formatted,
            is_error: false,
        })
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
        ToolContext {
            permission_mode: PermissionMode::Normal,
            project_root: tmp.path().to_path_buf(),
            ..ToolContext::new(tmp.path().to_path_buf())
        }
    }

    fn write_file(dir: &std::path::Path, name: &str, content: &str) {
        let path = dir.join(name);
        let parent = path.parent().unwrap();
        std::fs::create_dir_all(parent).unwrap();
        let mut f = std::fs::File::create(&path).unwrap();
        f.write_all(content.as_bytes()).unwrap();
    }

    #[tokio::test(flavor = "current_thread")]
    async fn reads_file_with_line_numbers() {
        let tmp = TempDir::new().unwrap();
        write_file(tmp.path(), "test.md", "line one\nline two\nline three");
        let ctx = test_ctx(&tmp);
        let out = ReadTool
            .call(json!({"file_path": "test.md"}), &ctx)
            .await
            .unwrap();
        assert!(out.content.contains("1\tline one"));
        assert!(out.content.contains("2\tline two"));
        assert!(out.content.contains("3\tline three"));
    }

    #[tokio::test(flavor = "current_thread")]
    async fn offset_limit_reads_subset() {
        let tmp = TempDir::new().unwrap();
        let lines: Vec<String> = (1..=20).map(|i| format!("line {i}")).collect();
        write_file(tmp.path(), "test.md", &lines.join("\n"));
        let ctx = test_ctx(&tmp);
        let out = ReadTool
            .call(json!({"file_path": "test.md", "offset": 5, "limit": 3}), &ctx)
            .await
            .unwrap();
        assert!(out.content.contains("5\tline 5"));
        assert!(out.content.contains("7\tline 7"));
        assert!(!out.content.contains("4\tline 4"));
        assert!(!out.content.contains("8\tline 8"));
    }

    #[tokio::test(flavor = "current_thread")]
    async fn empty_file_returns_warning() {
        let tmp = TempDir::new().unwrap();
        write_file(tmp.path(), "empty.md", "");
        let ctx = test_ctx(&tmp);
        let out = ReadTool
            .call(json!({"file_path": "empty.md"}), &ctx)
            .await
            .unwrap();
        assert!(out.content.contains("contents are empty"));
    }

    #[tokio::test(flavor = "current_thread")]
    async fn offset_beyond_file_returns_warning() {
        let tmp = TempDir::new().unwrap();
        write_file(tmp.path(), "short.md", "only one line");
        let ctx = test_ctx(&tmp);
        let out = ReadTool
            .call(json!({"file_path": "short.md", "offset": 10}), &ctx)
            .await
            .unwrap();
        assert!(out.content.contains("has only 1 lines"));
        assert!(out.content.contains("offset is 10"));
    }

    #[tokio::test(flavor = "current_thread")]
    async fn large_file_errors_on_full_read() {
        let tmp = TempDir::new().unwrap();
        let big_line = "a".repeat(1000);
        let mut content = String::new();
        for _ in 0..400 {
            content.push_str(&big_line);
            content.push('\n');
        }
        write_file(tmp.path(), "big.md", &content);
        let ctx = test_ctx(&tmp);
        let err = ReadTool
            .call(json!({"file_path": "big.md"}), &ctx)
            .await
            .unwrap_err();
        assert!(err.to_string().contains("exceeds maximum allowed size"));
    }

    #[tokio::test(flavor = "current_thread")]
    async fn large_file_with_limit_succeeds() {
        let tmp = TempDir::new().unwrap();
        let big_line = "a".repeat(1000);
        let mut content = String::new();
        for _ in 0..400 {
            content.push_str(&big_line);
            content.push('\n');
        }
        write_file(tmp.path(), "big.md", &content);
        let ctx = test_ctx(&tmp);
        let out = ReadTool
            .call(json!({"file_path": "big.md", "offset": 1, "limit": 5}), &ctx)
            .await
            .unwrap();
        assert!(out.content.contains("1\t"));
        assert!(out.content.contains("5\t"));
    }

    #[tokio::test(flavor = "current_thread")]
    async fn dedup_returns_stub_on_same_mtime() {
        let tmp = TempDir::new().unwrap();
        write_file(tmp.path(), "dup.md", "hello\nworld");
        let mut ctx = test_ctx(&tmp);
        ctx.read_file_cache = Some(Arc::new(dashmap::DashMap::new()));

        let out1 = ReadTool
            .call(json!({"file_path": "dup.md"}), &ctx)
            .await
            .unwrap();
        assert!(out1.content.contains("hello"));

        let out2 = ReadTool
            .call(json!({"file_path": "dup.md"}), &ctx)
            .await
            .unwrap();
        assert!(out2.content.contains("has not been changed since last read"));
    }

    #[tokio::test(flavor = "current_thread")]
    async fn write_refresh_cache_does_not_dedup_read() {
        use crate::ReadCacheSource;

        let tmp = TempDir::new().unwrap();
        write_file(tmp.path(), "edited.md", "after edit");
        let mut ctx = test_ctx(&tmp);
        ctx.read_file_cache = Some(Arc::new(dashmap::DashMap::new()));
        let full = ctx.resolve_path("edited.md");
        let meta = std::fs::metadata(&full).unwrap();
        ctx.store_read_cache(
            &full,
            ReadCacheEntry {
                mtime_secs: crate::file_mtime_secs(&meta),
                raw_content: "after edit".into(),
                offset: None,
                limit: None,
                total_lines: 1,
                source: ReadCacheSource::WriteRefresh,
            },
        );

        let out = ReadTool
            .call(json!({"file_path": "edited.md"}), &ctx)
            .await
            .unwrap();
        assert!(out.content.contains("after edit"));
        assert!(!out.content.contains("has not been changed since last read"));
    }
}
