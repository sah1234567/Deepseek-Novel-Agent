//! Session read cache: dedup, Edit read-before-write, partial/stale checks.

use crate::paths::{normalize_rel_path, optional_file_path};
use crate::ToolError;
use serde_json::Value;

pub const FILE_UNCHANGED_STUB: &str = "<system-reminder>File has not been changed since last read. Refer to the earlier Read/Tail tool_result in this conversation rather than re-reading.</system-reminder>";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReadCacheSource {
    /// Written by Read; eligible for Read dedup.
    Read,
    /// Written by Tail; eligible for Tail dedup.
    Tail,
    /// Written by Edit/Write refresh; not eligible for Read/Tail dedup (aligns with Claude Code).
    WriteRefresh,
}

impl ReadCacheSource {
    pub fn is_dedup_eligible(self) -> bool {
        matches!(self, ReadCacheSource::Read | ReadCacheSource::Tail)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReadCacheEntry {
    pub mtime_secs: u64,
    /// Raw file slice (no line-number prefix).
    pub raw_content: String,
    /// 1-indexed start line; `None` with `limit: None` = full file read.
    pub offset: Option<usize>,
    pub limit: Option<usize>,
    pub total_lines: usize,
    pub source: ReadCacheSource,
}

impl ReadCacheEntry {
    pub fn is_full_read(&self) -> bool {
        self.offset.is_none() && self.limit.is_none()
    }

    pub fn same_range(&self, offset: Option<usize>, limit: Option<usize>) -> bool {
        self.offset == offset && self.limit == limit
    }

    pub fn covers_edit_target(&self, old_string: &str) -> bool {
        if old_string.is_empty() {
            return true;
        }
        if self.is_full_read() {
            return true;
        }
        self.raw_content.contains(old_string)
    }

    /// Reject Write/Edit when disk mtime advanced and cached slice no longer matches.
    pub fn check_fresh_for_disk(
        &self,
        mtime_secs: u64,
        disk_content: &str,
        action: &str,
    ) -> Result<(), ToolError> {
        if mtime_secs <= self.mtime_secs {
            return Ok(());
        }
        if self.is_full_read() && self.raw_content == disk_content {
            return Ok(());
        }
        Err(ToolError::Execution(format!(
            "File modified since last read. Read again before {action}."
        )))
    }
}

pub fn is_read_dedup_stub(content: &str) -> bool {
    content.contains("File has not been changed since last read")
        && content.contains("Refer to the earlier Read")
}

pub fn format_read_dedup_hint(
    tool_name: &str,
    file_path: &str,
    offset: Option<usize>,
    limit: Option<usize>,
    tail_lines: Option<usize>,
) -> String {
    let range = if tool_name == "Tail" {
        format!("tail:last_{}_lines", tail_lines.unwrap_or(80))
    } else {
        let (off, lim) = read_range_key(offset, limit);
        match (off, lim) {
            (None, None) => "full_file".into(),
            (Some(o), Some(l)) => format!("offset:{o} limit:{l}"),
            (Some(o), None) => format!("offset:{o}"),
            _ => "partial".into(),
        }
    };
    format!(
        "[read-dedup] path={file_path} range={range}\n\
         Disk unchanged; identical {tool_name} already in this conversation — do NOT retry same parameters.\n\
         Use earlier tool_result. After Edit/Write: Read/Tail the changed range once (mtime updates).\n\
         Long chapters: Grep → Read range / Tail; full Read only when ≤200 lines."
    )
}

pub fn format_read_dedup_hint_from_input(tool_name: &str, input: &Value) -> Option<String> {
    let path = normalize_rel_path(&optional_file_path(input)?);
    let offset = input
        .get("offset")
        .and_then(|v| v.as_u64())
        .map(|n| n as usize);
    let limit = input
        .get("limit")
        .and_then(|v| v.as_u64())
        .map(|n| n as usize);
    let tail_lines = input
        .get("lines")
        .and_then(|v| v.as_u64())
        .map(|n| n as usize);
    let read_offset = if tool_name == "Read" {
        Some(offset.unwrap_or(1))
    } else {
        None
    };
    Some(format_read_dedup_hint(
        tool_name,
        &path,
        read_offset,
        if tool_name == "Read" { limit } else { None },
        if tool_name == "Tail" { Some(tail_lines.unwrap_or(80)) } else { None },
    ))
}

pub fn file_mtime_secs(meta: &std::fs::Metadata) -> u64 {
    meta.modified()
        .ok()
        .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

pub fn add_line_numbers(content: &str, start_line: usize) -> String {
    if content.is_empty() {
        return String::new();
    }
    content
        .lines()
        .enumerate()
        .map(|(i, line)| format!("{}\t{}", i + start_line, line))
        .collect::<Vec<_>>()
        .join("\n")
}

pub fn read_range_key(offset: Option<usize>, limit: Option<usize>) -> (Option<usize>, Option<usize>) {
    if limit.is_some() {
        (offset, limit)
    } else if offset.is_some_and(|o| o <= 1) {
        (None, None)
    } else {
        (offset, limit)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn entry(source: ReadCacheSource, off: Option<usize>, lim: Option<usize>) -> ReadCacheEntry {
        ReadCacheEntry {
            mtime_secs: 1,
            raw_content: "hello".into(),
            offset: off,
            limit: lim,
            total_lines: 10,
            source,
        }
    }

    #[test]
    fn partial_covers_edit_in_slice() {
        let e = ReadCacheEntry {
            mtime_secs: 1,
            raw_content: "hello world".into(),
            offset: Some(10),
            limit: Some(5),
            total_lines: 20,
            source: ReadCacheSource::Read,
        };
        assert!(e.covers_edit_target("hello"));
        assert!(!e.covers_edit_target("missing"));
    }

    #[test]
    fn full_read_covers_any_target() {
        let e = ReadCacheEntry {
            mtime_secs: 1,
            raw_content: "x".into(),
            offset: None,
            limit: None,
            total_lines: 1,
            source: ReadCacheSource::Read,
        };
        assert!(e.covers_edit_target("anything"));
    }

    #[test]
    fn write_refresh_not_dedup_eligible() {
        assert!(!ReadCacheSource::WriteRefresh.is_dedup_eligible());
        assert!(ReadCacheSource::Read.is_dedup_eligible());
    }

    #[test]
    fn is_read_dedup_stub_detects_message() {
        assert!(is_read_dedup_stub(FILE_UNCHANGED_STUB));
        assert!(!is_read_dedup_stub("1\tline"));
    }

    #[test]
    fn format_read_dedup_hint_read_range() {
        let h = format_read_dedup_hint("Read", "chapters/ch01.md", Some(5), Some(8), None);
        assert!(h.contains("path=chapters/ch01.md"));
        assert!(h.contains("offset:5 limit:8"));
    }

    #[test]
    fn format_read_dedup_hint_tail() {
        let h = format_read_dedup_hint("Tail", "chapters/ch01.md", None, None, Some(100));
        assert!(h.contains("tail:last_100_lines"));
    }

    #[test]
    fn dedup_eligibility_by_source() {
        assert!(entry(ReadCacheSource::Read, None, None).source.is_dedup_eligible());
        assert!(!entry(ReadCacheSource::WriteRefresh, None, None).source.is_dedup_eligible());
    }
}
