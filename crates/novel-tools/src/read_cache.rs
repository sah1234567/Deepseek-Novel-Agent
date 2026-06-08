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
    /// Cache slice patched after Edit; not dedup-eligible until the next Read/Tail.
    EditPatched,
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
    /// Span the model has seen via a persisted Read/Tail tool_result (Edit R1 uses this).
    pub transcript_committed: bool,
    pub committed_offset: Option<usize>,
    pub committed_limit: Option<usize>,
}

impl ReadCacheEntry {
    pub fn is_full_read(&self) -> bool {
        self.offset.is_none() && self.limit.is_none()
    }

    /// Mark the current cache window as visible to the model (after tool_result is in transcript).
    pub fn commit_to_transcript(&mut self) {
        self.transcript_committed = true;
        self.committed_offset = self.offset;
        self.committed_limit = self.limit;
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

    /// 1-indexed inclusive line span for a partial Read/Tail (`None` = full file).
    pub fn cached_line_span(&self) -> Option<(usize, usize)> {
        line_span_from(
            self.offset,
            self.limit,
            self.total_lines,
            self.is_full_read(),
        )
    }

    /// Line span the model has seen in transcript; `None` if partial and not yet committed.
    pub fn committed_line_span(&self) -> Option<(usize, usize)> {
        if !self.transcript_committed {
            return None;
        }
        line_span_from(
            self.committed_offset,
            self.committed_limit,
            self.total_lines,
            self.committed_offset.is_none()
                && self.committed_limit.is_none()
                && self.is_full_read(),
        )
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

fn line_span_from(
    offset: Option<usize>,
    limit: Option<usize>,
    total_lines: usize,
    is_full: bool,
) -> Option<(usize, usize)> {
    if is_full {
        return None;
    }
    match (offset, limit) {
        (Some(start), Some(lim)) => {
            let end = start.saturating_add(lim).saturating_sub(1);
            Some((start, end.min(total_lines.max(start))))
        }
        (Some(start), None) => Some((start, total_lines.max(start))),
        _ => None,
    }
}

/// 1-indexed line of the first `needle` occurrence in `haystack` (exported for tests/tooling).
#[allow(dead_code)]
pub fn line_number_1_indexed(haystack: &str, needle: &str) -> Option<usize> {
    if needle.is_empty() {
        return Some(1);
    }
    let idx = haystack.find(needle)?;
    Some(haystack[..idx].bytes().filter(|b| *b == b'\n').count() + 1)
}

/// Suggest a Read window around `line` respecting read-economy `max_lines`.
pub fn suggest_read_window(line: usize, total_lines: usize, max_lines: usize) -> (usize, usize) {
    let total = total_lines.max(1);
    let line = line.clamp(1, total);
    let pad = 5usize;
    let mut start = line.saturating_sub(pad).max(1);
    let mut end = (line + pad).min(total);
    let mut span = end.saturating_sub(start) + 1;
    if span > max_lines {
        start = line.saturating_sub(max_lines / 2).max(1);
        end = (start + max_lines - 1).min(total);
        start = end.saturating_sub(max_lines - 1).max(1);
        span = end.saturating_sub(start) + 1;
    }
    (
        start,
        span.min(max_lines).min(total.saturating_sub(start) + 1),
    )
}

/// Inclusive 1-indexed line span union.
pub(crate) fn union_line_span(a: (usize, usize), b: (usize, usize)) -> (usize, usize) {
    (a.0.min(b.0), a.1.max(b.1))
}

/// Extract `limit` lines starting at 1-indexed `start` from full file text.
pub(crate) fn extract_lines_1_indexed(content: &str, start: usize, limit: usize) -> String {
    let lines: Vec<&str> = content.lines().collect();
    let start_idx = start.saturating_sub(1);
    if start_idx >= lines.len() {
        return String::new();
    }
    let end = (start_idx + limit).min(lines.len());
    lines[start_idx..end].join("\n")
}

fn should_merge_partial_reads(existing: &ReadCacheEntry, incoming: &ReadCacheEntry) -> bool {
    incoming.mtime_secs == existing.mtime_secs
        && !incoming.is_full_read()
        && !existing.is_full_read()
        && existing.source.is_dedup_eligible()
        && incoming.source.is_dedup_eligible()
}

/// Merge partial Read/Tail windows on store (same mtime); re-slice from disk when possible.
pub(crate) fn merge_read_cache_on_store(
    existing: Option<&ReadCacheEntry>,
    incoming: ReadCacheEntry,
    rel_path: &str,
    disk_full: Option<&str>,
    premerged_raw: Option<&str>,
) -> Result<ReadCacheEntry, ToolError> {
    let Some(existing) = existing else {
        return Ok(incoming);
    };
    if !should_merge_partial_reads(existing, &incoming) {
        return Ok(incoming);
    }
    let Some(existing_span) = existing.cached_line_span() else {
        return Ok(incoming);
    };
    let Some(incoming_span) = incoming.cached_line_span() else {
        return Ok(incoming);
    };
    let (union_start, union_end) = union_line_span(existing_span, incoming_span);
    let union_limit = union_end.saturating_sub(union_start).saturating_add(1);

    crate::read_economy::read_pre_check(rel_path, Some(union_limit), incoming.total_lines)?;

    let raw = if let Some(disk) = disk_full {
        extract_lines_1_indexed(disk, union_start, union_limit)
    } else if let Some(pre) = premerged_raw {
        pre.to_string()
    } else {
        return Ok(incoming);
    };

    Ok(ReadCacheEntry {
        mtime_secs: incoming.mtime_secs,
        raw_content: raw,
        offset: Some(union_start),
        limit: Some(union_limit),
        total_lines: incoming.total_lines,
        source: incoming.source,
        transcript_committed: existing.transcript_committed,
        committed_offset: existing.committed_offset,
        committed_limit: existing.committed_limit,
    })
}

/// All 1-indexed inclusive line spans for each `needle` occurrence in `haystack`.
pub(crate) fn match_line_spans(haystack: &str, needle: &str) -> Vec<(usize, usize)> {
    if needle.is_empty() {
        return vec![(1, 1)];
    }
    let needle_lines = needle.lines().count().max(1);
    haystack
        .match_indices(needle)
        .map(|(idx, _)| {
            let start = haystack[..idx].bytes().filter(|b| *b == b'\n').count() + 1;
            let end = start + needle_lines - 1;
            (start, end)
        })
        .collect()
}

pub(crate) fn spans_within_cached_span(spans: &[(usize, usize)], entry: &ReadCacheEntry) -> bool {
    let Some((cache_start, cache_end)) = entry.cached_line_span() else {
        return true;
    };
    spans
        .iter()
        .all(|&(s, e)| s >= cache_start && e <= cache_end)
}

fn spans_within_committed_span(spans: &[(usize, usize)], entry: &ReadCacheEntry) -> bool {
    if entry.is_full_read() {
        return entry.transcript_committed;
    }
    let Some((cache_start, cache_end)) = entry.committed_line_span() else {
        return false;
    };
    spans
        .iter()
        .all(|&(s, e)| s >= cache_start && e <= cache_end)
}

fn edit_span_violation_error(entry: &ReadCacheEntry, line: usize, rel_path: &str) -> ToolError {
    let max = crate::read_economy::max_lines_for_path(rel_path)
        .unwrap_or(crate::read_economy::CHAPTER_MAX_LINES);
    let (offset, limit) = suggest_read_window(line, entry.total_lines.max(line), max);
    let read_span = entry
        .committed_line_span()
        .or_else(|| entry.cached_line_span())
        .map(|(s, e)| format!("{s}–{e}"))
        .unwrap_or_else(|| "?".into());
    ToolError::Execution(format!(
        "Edit target not in the read slice (only read a portion of this file). \
         old_string first matches at line {line}; editable lines (seen in conversation) {read_span}. \
         Re-Read with offset={offset} limit={limit} (≤{max} lines for this path), then retry Edit."
    ))
}

fn edit_pending_transcript_error(line: usize) -> ToolError {
    ToolError::Execution(format!(
        "Edit at line {line} was not applied: this line is only covered by a Read/Tail result \
         not yet in the conversation (older read cache does not include it). \
         Retry Edit on your next message; skip a new Read if the file mtime is unchanged."
    ))
}

/// Shared when `old_string` is not an exact substring of on-disk file bytes.
pub fn edit_old_string_not_on_disk_error(old_string: &str) -> ToolError {
    let preview: String = old_string.chars().take(80).collect();
    ToolError::Execution(format!(
        "old_string not found on disk (exact byte match required). \
         This is not a read-cache staleness issue. Preview: {preview}. \
         Grep a distinctive phrase, Read offset/limit around the match, then copy from tool_result \
         (no line-number prefix; do not paraphrase audit-report quotes)."
    ))
}

/// Edit guard: all affected match line spans must lie within cached partial span.
pub fn verify_edit_against_read_cache(
    entry: Option<&ReadCacheEntry>,
    old_string: &str,
    disk_content: &str,
    rel_path: &str,
    replace_all: bool,
) -> Result<(), ToolError> {
    let Some(entry) = entry else {
        return Ok(());
    };

    if !disk_content.contains(old_string) {
        return Err(edit_old_string_not_on_disk_error(old_string));
    }

    if entry.is_full_read() {
        return if entry.transcript_committed {
            Ok(())
        } else {
            Err(edit_pending_transcript_error(
                line_number_1_indexed(disk_content, old_string).unwrap_or(1),
            ))
        };
    }

    let spans = match_line_spans(disk_content, old_string);
    let check = if replace_all {
        spans.as_slice()
    } else {
        spans.first().map(std::slice::from_ref).unwrap_or(&[])
    };

    if spans_within_committed_span(check, entry) {
        return Ok(());
    }

    if spans_within_cached_span(check, entry) {
        let line = check
            .iter()
            .find(|&&(s, e)| {
                entry
                    .committed_line_span()
                    .is_none_or(|(cs, ce)| s < cs || e > ce)
            })
            .map(|(s, _)| *s)
            .unwrap_or(check[0].0);
        return Err(edit_pending_transcript_error(line));
    }

    let line = check
        .iter()
        .find(|&&(s, e)| {
            entry
                .cached_line_span()
                .is_none_or(|(cs, ce)| s < cs || e > ce)
        })
        .map(|(s, _)| *s)
        .unwrap_or(check[0].0);

    Err(edit_span_violation_error(entry, line, rel_path))
}

/// After Edit: patch partial slice in place; full Read/Tail uses string replace; WriteRefresh → full file.
pub(crate) fn patch_read_cache_after_edit(
    entry: &mut ReadCacheEntry,
    updated_disk: &str,
    mtime_secs: u64,
    old_string: &str,
    new_string: &str,
    replace_all: bool,
    occurrences_replaced: usize,
) {
    if entry.source == ReadCacheSource::WriteRefresh {
        entry.mtime_secs = mtime_secs;
        entry.raw_content = updated_disk.to_string();
        entry.offset = None;
        entry.limit = None;
        entry.total_lines = updated_disk.lines().count();
        return;
    }

    let old_lines = old_string.lines().count().max(1);
    let new_lines = new_string.lines().count().max(1);
    let delta = (new_lines as i64 - old_lines as i64) * occurrences_replaced as i64;

    if entry.is_full_read() {
        entry.raw_content = if replace_all {
            entry.raw_content.replace(old_string, new_string)
        } else {
            entry.raw_content.replacen(old_string, new_string, 1)
        };
        entry.total_lines = updated_disk.lines().count();
        entry.mtime_secs = mtime_secs;
        entry.source = ReadCacheSource::EditPatched;
        return;
    }

    // Re-slice from disk after edit — avoids drift when edits change line counts above the window.
    entry.total_lines = (entry.total_lines as i64 + delta).max(0) as usize;
    let new_limit = entry
        .limit
        .map(|l| ((l as i64) + delta).max(1) as usize)
        .unwrap_or(1);
    let offset = entry.offset.unwrap_or(1);
    entry.raw_content = extract_lines_1_indexed(updated_disk, offset, new_limit);
    entry.limit = Some(new_limit);
    if entry.transcript_committed {
        entry.committed_limit = Some(new_limit);
    }
    entry.mtime_secs = mtime_secs;
    entry.source = ReadCacheSource::EditPatched;
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
         Use earlier tool_result. Partial Read windows merge on repeat Read/Tail (same mtime).\n\
         After Edit on this slice: cache is patched but conversation may be stale — use next Edit with updated text, or Read with different offset/limit (not same params).\n\
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
        if tool_name == "Tail" {
            Some(tail_lines.unwrap_or(80))
        } else {
            None
        },
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

pub fn read_range_key(
    offset: Option<usize>,
    limit: Option<usize>,
) -> (Option<usize>, Option<usize>) {
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
            transcript_committed: false,
            committed_offset: None,
            committed_limit: None,
        }
    }

    fn lines_disk(n: usize) -> String {
        (1..=n)
            .map(|i| format!("line {i}"))
            .collect::<Vec<_>>()
            .join("\n")
    }

    fn partial_entry(
        disk: &str,
        offset: usize,
        limit: usize,
        source: ReadCacheSource,
    ) -> ReadCacheEntry {
        let mut entry = ReadCacheEntry {
            mtime_secs: 1,
            raw_content: extract_lines_1_indexed(disk, offset, limit),
            offset: Some(offset),
            limit: Some(limit),
            total_lines: disk.lines().count(),
            source,
            transcript_committed: false,
            committed_offset: None,
            committed_limit: None,
        };
        entry.commit_to_transcript();
        entry
    }

    fn assert_cached_line_span(entry: &ReadCacheEntry, start: usize, end: usize) {
        assert_eq!(entry.cached_line_span(), Some((start, end)));
        assert_eq!(entry.offset, Some(start));
        assert_eq!(
            entry.limit,
            Some(end.saturating_sub(start).saturating_add(1))
        );
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
            transcript_committed: false,
            committed_offset: None,
            committed_limit: None,
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
            transcript_committed: false,
            committed_offset: None,
            committed_limit: None,
        };
        assert!(e.covers_edit_target("anything"));
    }

    #[test]
    fn write_refresh_not_dedup_eligible() {
        assert!(!ReadCacheSource::WriteRefresh.is_dedup_eligible());
        assert!(!ReadCacheSource::EditPatched.is_dedup_eligible());
        assert!(ReadCacheSource::Read.is_dedup_eligible());
    }

    #[test]
    fn verify_edit_not_on_disk_does_not_blame_read_slice() {
        let disk = lines_disk(50);
        let entry = partial_entry(&disk, 33, 11, ReadCacheSource::Read);
        let err = verify_edit_against_read_cache(
            Some(&entry),
            "line 999",
            &disk,
            "chapters/ch01.md",
            false,
        )
        .unwrap_err()
        .to_string();
        assert!(err.contains("not found on disk"));
        assert!(err.contains("not a read-cache staleness"));
        assert!(!err.contains("only read a portion"));
    }

    #[test]
    fn is_read_dedup_stub_detects_message() {
        assert!(is_read_dedup_stub(FILE_UNCHANGED_STUB));
        assert!(!is_read_dedup_stub("1\tline"));
    }

    #[test]
    fn line_number_1_indexed_counts_from_start() {
        assert_eq!(line_number_1_indexed("a\nb\nc", "b"), Some(2));
    }

    #[test]
    fn union_span_merges_80_100_and_50_90() {
        let disk = lines_disk(100);
        let mut existing = partial_entry(&disk, 80, 21, ReadCacheSource::Read);
        existing.mtime_secs = 5;
        let incoming = ReadCacheEntry {
            mtime_secs: 5,
            raw_content: extract_lines_1_indexed(&disk, 50, 41),
            offset: Some(50),
            limit: Some(41),
            total_lines: 100,
            source: ReadCacheSource::Read,
            transcript_committed: false,
            committed_offset: None,
            committed_limit: None,
        };
        let merged = merge_read_cache_on_store(
            Some(&existing),
            incoming,
            "chapters/ch01.md",
            Some(&disk),
            None,
        )
        .unwrap();
        assert_eq!(merged.offset, Some(50));
        assert_eq!(merged.limit, Some(51));
        assert!(merged.raw_content.contains("line 50"));
        assert!(merged.raw_content.contains("line 100"));
    }

    #[test]
    fn merge_skips_when_mtime_differs() {
        let disk = lines_disk(10);
        let existing = ReadCacheEntry {
            mtime_secs: 1,
            raw_content: "line 8".into(),
            offset: Some(8),
            limit: Some(3),
            total_lines: 10,
            source: ReadCacheSource::Read,
            transcript_committed: false,
            committed_offset: None,
            committed_limit: None,
        };
        let incoming = ReadCacheEntry {
            mtime_secs: 2,
            raw_content: "line 2".into(),
            offset: Some(2),
            limit: Some(3),
            total_lines: 10,
            source: ReadCacheSource::Read,
            transcript_committed: false,
            committed_offset: None,
            committed_limit: None,
        };
        let merged = merge_read_cache_on_store(
            Some(&existing),
            incoming.clone(),
            "chapters/ch01.md",
            Some(&disk),
            None,
        )
        .unwrap();
        assert_eq!(merged.offset, incoming.offset);
    }

    #[test]
    fn verify_edit_allows_target_line_inside_cached_span() {
        let disk = lines_disk(50);
        let entry = partial_entry(&disk, 33, 11, ReadCacheSource::Read);
        assert!(verify_edit_against_read_cache(
            Some(&entry),
            "line 35",
            &disk,
            "chapters/ch01.md",
            false
        )
        .is_ok());
    }

    #[test]
    fn verify_edit_suggests_read_window_when_outside_span() {
        let disk = lines_disk(60);
        let entry = partial_entry(&disk, 1, 5, ReadCacheSource::Read);
        let err = verify_edit_against_read_cache(
            Some(&entry),
            "line 50",
            &disk,
            "chapters/ch01.md",
            false,
        )
        .unwrap_err()
        .to_string();
        assert!(err.contains("offset="));
        assert!(err.contains("limit="));
    }

    #[test]
    fn replace_all_fails_when_one_match_outside_span() {
        let disk = "alpha\nbeta\nalpha\n".to_string();
        let entry = partial_entry(&disk, 2, 1, ReadCacheSource::Read);
        assert!(verify_edit_against_read_cache(
            Some(&entry),
            "alpha",
            &disk,
            "chapters/ch01.md",
            true
        )
        .is_err());
    }

    #[test]
    fn verify_edit_rejects_cache_span_not_yet_in_transcript() {
        let disk = lines_disk(50);
        let mut entry = ReadCacheEntry {
            mtime_secs: 1,
            raw_content: extract_lines_1_indexed(&disk, 33, 11),
            offset: Some(33),
            limit: Some(11),
            total_lines: 50,
            source: ReadCacheSource::Read,
            transcript_committed: false,
            committed_offset: None,
            committed_limit: None,
        };
        let err = verify_edit_against_read_cache(
            Some(&entry),
            "line 35",
            &disk,
            "chapters/ch01.md",
            false,
        )
        .unwrap_err()
        .to_string();
        assert!(err.contains("was not applied"));
        assert!(err.contains("not yet in the conversation"));
        entry.commit_to_transcript();
        assert!(verify_edit_against_read_cache(
            Some(&entry),
            "line 35",
            &disk,
            "chapters/ch01.md",
            false,
        )
        .is_ok());
    }

    #[test]
    fn patch_partial_sets_edit_patched_source() {
        let disk = lines_disk(50);
        let mut entry = partial_entry(&disk, 33, 11, ReadCacheSource::Read);
        let updated = disk.replacen("line 35", "line 35 edited", 1);
        patch_read_cache_after_edit(
            &mut entry,
            &updated,
            2,
            "line 35",
            "line 35 edited",
            false,
            1,
        );
        assert_eq!(entry.source, ReadCacheSource::EditPatched);
        assert_eq!(entry.offset, Some(33));
        assert_eq!(entry.total_lines, 50);
        assert_eq!(entry.limit, Some(11));
        assert_cached_line_span(&entry, 33, 43);
        assert!(entry.raw_content.contains("line 35 edited"));
    }

    #[test]
    fn patch_zero_delta_multiline_preserves_cached_line_span() {
        let disk = lines_disk(50);
        let mut entry = partial_entry(&disk, 33, 11, ReadCacheSource::Read);
        let old = "line 35\nline 36";
        let new = "edited 35\nedited 36";
        let updated = disk.replacen(old, new, 1);
        patch_read_cache_after_edit(&mut entry, &updated, 2, old, new, false, 1);
        assert_eq!(entry.total_lines, 50);
        assert_cached_line_span(&entry, 33, 43);
        assert!(entry.raw_content.contains("edited 35"));
        assert_eq!(entry.raw_content.lines().count(), 11);
    }

    #[test]
    fn patch_replace_all_zero_delta_preserves_cached_line_span() {
        let mut lines: Vec<String> = (1..=50).map(|n| format!("line {n}")).collect();
        lines[34] = "TOK mid".into();
        lines[39] = "TOK end".into();
        let disk = lines.join("\n");
        let mut entry = partial_entry(&disk, 33, 11, ReadCacheSource::Read);
        let old = "TOK";
        let new = "TOK2";
        let updated = disk.replace(old, new);
        patch_read_cache_after_edit(&mut entry, &updated, 2, old, new, true, 2);
        assert_eq!(entry.total_lines, 50);
        assert_cached_line_span(&entry, 33, 43);
        assert!(entry.raw_content.contains("TOK2 mid"));
        assert!(entry.raw_content.contains("TOK2 end"));
        assert_eq!(entry.raw_content.lines().count(), 11);
    }

    #[test]
    fn patch_partial_increases_total_lines_and_limit() {
        let disk = lines_disk(50);
        let mut entry = partial_entry(&disk, 33, 11, ReadCacheSource::Read);
        let new_block = "line 35 expanded\nline 35 extra\nline 35 more";
        let updated = disk.replacen("line 35", new_block, 1);
        patch_read_cache_after_edit(&mut entry, &updated, 2, "line 35", new_block, false, 1);
        assert_eq!(entry.total_lines, 52);
        assert_eq!(entry.limit, Some(13));
        assert_eq!(entry.offset, Some(33));
        assert!(entry.raw_content.contains("line 35 expanded"));
        assert_eq!(entry.raw_content.lines().count(), 13);
    }

    #[test]
    fn patch_partial_decreases_total_lines_and_limit() {
        let disk = lines_disk(50);
        let mut entry = partial_entry(&disk, 33, 11, ReadCacheSource::Read);
        let old = "line 35\nline 36\nline 37";
        let new = "line 35-37 merged";
        let updated = disk.replacen(old, new, 1);
        patch_read_cache_after_edit(&mut entry, &updated, 2, old, new, false, 1);
        assert_eq!(entry.total_lines, 48);
        assert_eq!(entry.limit, Some(9));
        assert!(entry.raw_content.contains("line 35-37 merged"));
        assert!(!entry.raw_content.contains("line 36\n"));
    }

    #[test]
    fn patch_replace_all_accumulates_delta_per_occurrence() {
        let mut lines: Vec<String> = (1..=50).map(|n| format!("line {n}")).collect();
        lines[34] = "TAG mid".into();
        lines[39] = "TAG end".into();
        let disk = lines.join("\n");
        let mut entry = partial_entry(&disk, 33, 11, ReadCacheSource::Read);
        let old = "TAG";
        let new = "TAG\nextra";
        let updated = disk.replace(old, new);
        patch_read_cache_after_edit(&mut entry, &updated, 2, old, new, true, 2);
        assert_eq!(entry.total_lines, 52);
        assert_eq!(entry.limit, Some(13));
        assert!(entry.raw_content.matches("extra").count() >= 2);
    }

    #[test]
    fn patch_tail_source_updates_line_counts() {
        let disk = lines_disk(30);
        let mut entry = partial_entry(&disk, 21, 10, ReadCacheSource::Tail);
        let new_block = "line 25a\nline 25b";
        let updated = disk.replacen("line 25", new_block, 1);
        patch_read_cache_after_edit(&mut entry, &updated, 2, "line 25", new_block, false, 1);
        assert_eq!(entry.source, ReadCacheSource::EditPatched);
        assert_eq!(entry.total_lines, 31);
        assert_eq!(entry.limit, Some(11));
    }

    #[test]
    fn patch_write_refresh_replaces_full_file() {
        let mut entry = ReadCacheEntry {
            mtime_secs: 1,
            raw_content: "a\nb\nc".into(),
            offset: None,
            limit: None,
            total_lines: 3,
            source: ReadCacheSource::WriteRefresh,
            transcript_committed: true,
            committed_offset: None,
            committed_limit: None,
        };
        let updated = "a\nb\nc\nd";
        patch_read_cache_after_edit(&mut entry, updated, 3, "c", "c\nd", false, 1);
        assert_eq!(entry.total_lines, 4);
        assert_eq!(entry.raw_content, updated);
        assert!(entry.is_full_read());
        assert_eq!(entry.source, ReadCacheSource::WriteRefresh);
    }

    #[test]
    fn patch_full_read_recounts_from_disk() {
        let disk = "a\nb\nc";
        let mut entry = ReadCacheEntry {
            mtime_secs: 1,
            raw_content: disk.into(),
            offset: None,
            limit: None,
            total_lines: 3,
            source: ReadCacheSource::Read,
            transcript_committed: true,
            committed_offset: None,
            committed_limit: None,
        };
        let updated = "a\nb\nc\nd";
        patch_read_cache_after_edit(&mut entry, updated, 2, "c", "c\nd", false, 1);
        assert_eq!(entry.total_lines, 4);
        assert_eq!(entry.raw_content, updated);
    }

    #[test]
    fn patch_sequential_edits_accumulate_line_delta() {
        let disk = lines_disk(20);
        let mut entry = partial_entry(&disk, 5, 6, ReadCacheSource::Read);
        let updated1 = disk.replacen("line 6", "line 6a\nline 6b", 1);
        patch_read_cache_after_edit(
            &mut entry,
            &updated1,
            2,
            "line 6",
            "line 6a\nline 6b",
            false,
            1,
        );
        assert_eq!(entry.total_lines, 21);
        assert_eq!(entry.limit, Some(7));
        let updated2 = updated1.replacen("line 8", "line 8a\nline 8b\nline 8c", 1);
        patch_read_cache_after_edit(
            &mut entry,
            &updated2,
            2,
            "line 8",
            "line 8a\nline 8b\nline 8c",
            false,
            1,
        );
        assert_eq!(entry.total_lines, 23);
        assert_eq!(entry.limit, Some(9));
        assert!(entry.raw_content.contains("line 8c"));
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
        assert!(entry(ReadCacheSource::Read, None, None)
            .source
            .is_dedup_eligible());
        assert!(!entry(ReadCacheSource::WriteRefresh, None, None)
            .source
            .is_dedup_eligible());
        assert!(!entry(ReadCacheSource::EditPatched, None, None)
            .source
            .is_dedup_eligible());
    }
}
