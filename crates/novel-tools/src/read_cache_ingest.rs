//! Shared Read/Tail → session read cache ingest (live tools + rebuild).

use crate::paths::optional_file_path;
use crate::read_cache::{
    file_mtime_secs, read_range_key, ReadCacheEntry, ReadCacheSource, FAST_PATH_MAX_SIZE,
};
use crate::{ToolContext, ToolError};
use serde_json::Value;
use std::path::Path;

/// Disk content already read by the live Read/Tail tool — avoids a second full read on ingest.
pub(crate) struct IngestDiskPayload {
    pub mtime_secs: u64,
    pub file_len: u64,
    pub disk_full: String,
    pub raw_slice: String,
    pub total_lines: usize,
}

fn read_disk_slice(
    full: &Path,
    line_offset: usize,
    limit: Option<usize>,
) -> Result<(String, usize, usize, String), ToolError> {
    let metadata = std::fs::metadata(full).map_err(ToolError::Io)?;
    if metadata.len() == 0 {
        return Ok((String::new(), 0, 0, String::new()));
    }
    let disk_full = std::fs::read_to_string(full).map_err(ToolError::Io)?;
    let all_lines: Vec<&str> = disk_full.lines().collect();
    let total_lines = all_lines.len();
    if line_offset >= total_lines {
        return Ok((String::new(), 0, total_lines, disk_full));
    }
    let end = match limit {
        Some(lim) => (line_offset + lim).min(total_lines),
        None => total_lines,
    };
    let selected: Vec<&str> = all_lines[line_offset..end].to_vec();
    let raw = selected.join("\n");
    Ok((raw, selected.len(), total_lines, disk_full))
}

fn tail_window(total_lines: usize, lines_n: usize) -> (usize, usize, usize) {
    let take = lines_n.min(total_lines);
    let start_idx = total_lines.saturating_sub(take);
    let start_line = if take == 0 { 1 } else { start_idx + 1 };
    (start_line, take, start_idx)
}

fn store_read_cache_entry(
    ctx: &ToolContext,
    full_path: &Path,
    entry: ReadCacheEntry,
    disk_full: Option<&str>,
) -> Result<(), ToolError> {
    ctx.store_read_cache(full_path, entry, disk_full, None)
}

fn ingest_empty_file_cache(
    ctx: &ToolContext,
    full_path: &Path,
    mtime: u64,
    source: ReadCacheSource,
) -> Result<(), ToolError> {
    store_read_cache_entry(
        ctx,
        full_path,
        ReadCacheEntry {
            mtime_secs: mtime,
            raw_content: String::new(),
            offset: None,
            limit: None,
            total_lines: 0,
            source,
            transcript_committed: false,
            committed_spans: Vec::new(),
            committed_offset: None,
            committed_limit: None,
        },
        None,
    )
}

fn ingest_read_tool_cache(
    ctx: &ToolContext,
    full_path: &Path,
    input: &Value,
    mtime: u64,
    file_len: u64,
    source: ReadCacheSource,
    payload: Option<&IngestDiskPayload>,
) -> Result<(), ToolError> {
    let offset = input.get("offset").and_then(|v| v.as_u64()).unwrap_or(1) as usize;
    let limit = input
        .get("limit")
        .and_then(|v| v.as_u64())
        .map(|l| l as usize);
    let (raw, total_lines, disk_full) = if let Some(p) = payload {
        tracing::debug!(path = %full_path.display(), "read_cache ingest: reusing live Read disk payload");
        (p.raw_slice.clone(), p.total_lines, p.disk_full.clone())
    } else {
        let line_offset = if offset == 0 {
            0
        } else {
            offset.saturating_sub(1)
        };
        let (raw, _lc, total_lines, disk_full) = read_disk_slice(full_path, line_offset, limit)?;
        (raw, total_lines, disk_full)
    };
    let (cache_off, cache_lim) = read_range_key(
        if limit.is_some() || offset > 1 {
            Some(offset)
        } else {
            None
        },
        limit,
    );
    let disk_opt = if file_len < FAST_PATH_MAX_SIZE {
        Some(disk_full.as_str())
    } else {
        None
    };
    store_read_cache_entry(
        ctx,
        full_path,
        ReadCacheEntry {
            mtime_secs: mtime,
            raw_content: raw,
            offset: cache_off,
            limit: cache_lim,
            total_lines,
            source,
            transcript_committed: false,
            committed_spans: Vec::new(),
            committed_offset: None,
            committed_limit: None,
        },
        disk_opt,
    )
}

fn ingest_tail_tool_cache(
    ctx: &ToolContext,
    full_path: &Path,
    input: &Value,
    mtime: u64,
    source: ReadCacheSource,
    payload: Option<&IngestDiskPayload>,
) -> Result<(), ToolError> {
    let lines_n = input.get("lines").and_then(|v| v.as_u64()).unwrap_or(100) as usize;
    let (raw, total_lines, disk_full) = if let Some(p) = payload {
        tracing::debug!(path = %full_path.display(), "read_cache ingest: reusing live Tail disk payload");
        (p.raw_slice.clone(), p.total_lines, p.disk_full.clone())
    } else {
        let disk_full = std::fs::read_to_string(full_path).map_err(ToolError::Io)?;
        let total_lines = disk_full.lines().count();
        let (start_line, take, start_idx) = tail_window(total_lines, lines_n);
        let raw = disk_full
            .lines()
            .skip(start_idx)
            .take(take)
            .collect::<Vec<_>>()
            .join("\n");
        let _ = (start_line, take);
        (raw, total_lines, disk_full)
    };
    let (start_line, take, _) = tail_window(total_lines, lines_n);
    store_read_cache_entry(
        ctx,
        full_path,
        ReadCacheEntry {
            mtime_secs: mtime,
            raw_content: raw,
            offset: Some(start_line),
            limit: Some(take),
            total_lines,
            source,
            transcript_committed: false,
            committed_spans: Vec::new(),
            committed_offset: None,
            committed_limit: None,
        },
        Some(&disk_full),
    )
}

/// Read disk and store a Read/Tail cache entry (merge when `disk_full` supplied).
pub(crate) fn ingest_read_or_tail_into_cache(
    ctx: &ToolContext,
    tool_name: &str,
    full_path: &Path,
    input: &Value,
    source: ReadCacheSource,
    payload: Option<&IngestDiskPayload>,
) -> Result<(), ToolError> {
    let _ = optional_file_path(input).ok_or_else(|| {
        ToolError::Execution("ingest_read_or_tail_into_cache: missing file_path".into())
    })?;

    let (mtime, file_len) = if let Some(p) = payload {
        (p.mtime_secs, p.file_len)
    } else {
        let metadata = std::fs::metadata(full_path).map_err(ToolError::Io)?;
        (file_mtime_secs(&metadata), metadata.len())
    };

    if file_len == 0 {
        return ingest_empty_file_cache(ctx, full_path, mtime, source);
    }

    match tool_name {
        "Read" => ingest_read_tool_cache(ctx, full_path, input, mtime, file_len, source, payload),
        "Tail" => ingest_tail_tool_cache(ctx, full_path, input, mtime, source, payload),
        other => Err(ToolError::Execution(format!(
            "ingest_read_or_tail_into_cache: unsupported tool {other}"
        ))),
    }
}

pub(crate) fn is_tool_result_error(content: &str) -> bool {
    content.starts_with("Error:") || content.starts_with("Error ")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::PermissionMode;
    use std::sync::Arc;
    use tempfile::TempDir;

    #[test]
    fn tail_window_from_end() {
        let (start, take, idx) = tail_window(10, 3);
        assert_eq!((start, take, idx), (8, 3, 7));
    }

    #[test]
    fn detects_tool_error_prefix() {
        assert!(is_tool_result_error("Error: not found"));
        assert!(!is_tool_result_error("1\thello"));
    }

    fn test_ctx(tmp: &TempDir) -> ToolContext {
        let mut ctx = ToolContext::new(tmp.path().to_path_buf());
        ctx.read_file_cache = Some(Arc::new(dashmap::DashMap::new()));
        ctx.permission_mode = PermissionMode::Normal;
        ctx
    }

    #[test]
    fn ingest_reuses_payload_without_second_disk_read() {
        let tmp = TempDir::new().unwrap();
        let path = tmp.path().join("ch.md");
        let body = (1..=30)
            .map(|n| format!("line {n}"))
            .collect::<Vec<_>>()
            .join("\n");
        std::fs::write(&path, &body).unwrap();
        let ctx = test_ctx(&tmp);
        let input = serde_json::json!({"file_path": "ch.md", "offset": 5});
        let payload = IngestDiskPayload {
            mtime_secs: 1,
            file_len: body.len() as u64,
            disk_full: body.clone(),
            raw_slice: (5..=30)
                .map(|n| format!("line {n}"))
                .collect::<Vec<_>>()
                .join("\n"),
            total_lines: 30,
        };
        ingest_read_or_tail_into_cache(
            &ctx,
            "Read",
            &path,
            &input,
            ReadCacheSource::Read,
            Some(&payload),
        )
        .unwrap();
        let entry = ctx.read_cache_entry(&path).unwrap();
        assert_eq!(entry.offset, Some(5));
        assert_eq!(entry.limit, None);
        assert_eq!(entry.total_lines, 30);
        assert!(entry.raw_content.contains("line 30"));
    }
}
