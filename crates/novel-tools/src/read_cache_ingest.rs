//! Shared Read/Tail → session read cache ingest (live tools + rebuild).

use crate::paths::optional_file_path;
use crate::read_cache::{
    file_mtime_secs, read_range_key, ReadCacheEntry, ReadCacheSource, FAST_PATH_MAX_SIZE,
};
use crate::{ToolContext, ToolError, ToolRegistry};
use serde_json::Value;
use std::path::Path;

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

/// Read disk and store a Read/Tail cache entry (merge when `disk_full` supplied).
pub(crate) fn ingest_read_or_tail_into_cache(
    ctx: &ToolContext,
    registry: &ToolRegistry,
    tool_name: &str,
    full_path: &Path,
    input: &Value,
    source: ReadCacheSource,
) -> Result<(), ToolError> {
    let _ = registry;
    let rel = optional_file_path(input).ok_or_else(|| {
        ToolError::Execution("ingest_read_or_tail_into_cache: missing file_path".into())
    })?;
    let _rel = rel;

    let metadata = std::fs::metadata(full_path).map_err(ToolError::Io)?;
    let mtime = file_mtime_secs(&metadata);

    if metadata.len() == 0 {
        ctx.store_read_cache(
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
            None,
        )?;
        return Ok(());
    }

    match tool_name {
        "Read" => {
            let offset = input.get("offset").and_then(|v| v.as_u64()).unwrap_or(1) as usize;
            let limit = input
                .get("limit")
                .and_then(|v| v.as_u64())
                .map(|l| l as usize);
            let line_offset = if offset == 0 {
                0
            } else {
                offset.saturating_sub(1)
            };
            let (raw, _lc, total_lines, disk_full) =
                read_disk_slice(full_path, line_offset, limit)?;
            let (cache_off, cache_lim) = read_range_key(
                if limit.is_some() || offset > 1 {
                    Some(offset)
                } else {
                    None
                },
                limit,
            );
            let disk_opt = if metadata.len() < FAST_PATH_MAX_SIZE {
                Some(disk_full)
            } else {
                None
            };
            ctx.store_read_cache(
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
                disk_opt.as_deref(),
                None,
            )?;
        }
        "Tail" => {
            let lines_n = input.get("lines").and_then(|v| v.as_u64()).unwrap_or(100) as usize;
            let disk_full = std::fs::read_to_string(full_path).map_err(ToolError::Io)?;
            let total_lines = disk_full.lines().count();
            let (start_line, take, start_idx) = tail_window(total_lines, lines_n);
            let raw = disk_full
                .lines()
                .skip(start_idx)
                .collect::<Vec<_>>()
                .join("\n");
            ctx.store_read_cache(
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
                None,
            )?;
        }
        other => {
            return Err(ToolError::Execution(format!(
                "ingest_read_or_tail_into_cache: unsupported tool {other}"
            )));
        }
    }
    Ok(())
}

pub(crate) fn is_tool_result_error(content: &str) -> bool {
    content.starts_with("Error:") || content.starts_with("Error ")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detects_tool_error_prefix() {
        assert!(is_tool_result_error("Error: not found"));
        assert!(!is_tool_result_error("1\thello"));
    }
}
