//! Session read cache: dedup, Edit read-before-write, partial/stale checks.
//! Cached span (merge union) may be wider than transcript visibility; Edit R1 uses `committed_spans` only.
//! `replace_all` with session cache skips R1; post-edit cache becomes full file.

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
    /// Written by Edit/Write refresh; not eligible for Read/Tail dedup.
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
    /// Inclusive 1-indexed line spans present in transcript (disjoint ranges allowed).
    pub committed_spans: Vec<(usize, usize)>,
    /// Bounding box of `committed_spans` for display; not used for Edit R1 checks.
    pub committed_offset: Option<usize>,
    pub committed_limit: Option<usize>,
}

impl ReadCacheEntry {
    pub fn is_full_read(&self) -> bool {
        self.offset.is_none() && self.limit.is_none()
    }

    /// Mark the full cache window as committed (tests, Write refresh, full-file Read).
    pub fn commit_to_transcript(&mut self) {
        self.transcript_committed = true;
        if self.is_full_read() {
            self.committed_spans.clear();
            self.committed_offset = None;
            self.committed_limit = None;
        } else if let Some(span) = self.cached_line_span() {
            self.commit_span(span);
        }
    }

    /// Record one Read/Tail tool_result span (from tool input, not merged cache union).
    pub fn commit_span(&mut self, span: (usize, usize)) {
        self.transcript_committed = true;
        insert_and_coalesce_span(&mut self.committed_spans, span);
        sync_committed_bounding_fields(self);
    }

    pub fn same_range(&self, offset: Option<usize>, limit: Option<usize>) -> bool {
        self.offset == offset && self.limit == limit
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

    pub fn format_committed_spans(&self) -> String {
        if self.is_full_read() && self.transcript_committed {
            return "full file".into();
        }
        if self.committed_spans.is_empty() {
            return "?".into();
        }
        self.committed_spans
            .iter()
            .map(|(s, e)| format!("{s}–{e}"))
            .collect::<Vec<_>>()
            .join(", ")
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

fn committed_spans_bounding_box(spans: &[(usize, usize)]) -> Option<(usize, usize)> {
    if spans.is_empty() {
        return None;
    }
    let start = spans.iter().map(|(s, _)| *s).min()?;
    let end = spans.iter().map(|(_, e)| *e).max()?;
    Some((start, end))
}

fn sync_committed_bounding_fields(entry: &mut ReadCacheEntry) {
    if let Some((start, end)) = committed_spans_bounding_box(&entry.committed_spans) {
        entry.committed_offset = Some(start);
        entry.committed_limit = Some(end.saturating_sub(start).saturating_add(1));
    } else {
        entry.committed_offset = None;
        entry.committed_limit = None;
    }
}

fn coalesce_committed_spans(spans: &mut Vec<(usize, usize)>) {
    if spans.is_empty() {
        return;
    }
    spans.sort_by_key(|(s, _)| *s);
    let mut merged = vec![spans[0]];
    for &(s, e) in spans.iter().skip(1) {
        let last = merged.last_mut().expect("merged non-empty");
        if last.1.saturating_add(1) >= s {
            last.1 = last.1.max(e);
        } else {
            merged.push((s, e));
        }
    }
    *spans = merged;
}

fn insert_and_coalesce_span(spans: &mut Vec<(usize, usize)>, new: (usize, usize)) {
    spans.push(new);
    coalesce_committed_spans(spans);
}

fn shift_committed_spans_for_edit(
    spans: &mut Vec<(usize, usize)>,
    first_edit_line: usize,
    total_delta: i64,
) {
    if total_delta == 0 || spans.is_empty() {
        return;
    }
    for (start, end) in spans.iter_mut() {
        if *end < first_edit_line {
            continue;
        }
        if *start >= first_edit_line {
            *start = (*start as i64 + total_delta).max(1) as usize;
        }
        *end = (*end as i64 + total_delta).max(*start as i64) as usize;
    }
    coalesce_committed_spans(spans);
}

/// Inclusive line span for a persisted Read/Tail tool_result from tool input.
pub(crate) fn span_from_tool_input(
    registry: &crate::ToolRegistry,
    tool_name: &str,
    input: &Value,
    total_lines: usize,
) -> Option<(usize, usize)> {
    registry
        .get(tool_name)?
        .extract_read_span(input, total_lines)
}

fn line_number_1_indexed(haystack: &str, needle: &str) -> Option<usize> {
    if needle.is_empty() {
        return Some(1);
    }
    let idx = haystack.find(needle)?;
    Some(haystack[..idx].bytes().filter(|b| *b == b'\n').count() + 1)
}

/// Suggest a Read window around `line` respecting read-economy `max_lines`.
fn suggest_read_window(line: usize, total_lines: usize, max_lines: usize) -> (usize, usize) {
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
/// Read-economy limits apply per tool_result only, not to the merged cache union.
pub(crate) fn merge_read_cache_on_store(
    existing: Option<&ReadCacheEntry>,
    incoming: ReadCacheEntry,
    disk_full: Option<&str>,
    premerged_raw: Option<&str>,
) -> ReadCacheEntry {
    let Some(existing) = existing else {
        return incoming;
    };
    if !should_merge_partial_reads(existing, &incoming) {
        return incoming;
    }
    let Some(existing_span) = existing.cached_line_span() else {
        return incoming;
    };
    let Some(incoming_span) = incoming.cached_line_span() else {
        return incoming;
    };
    let (union_start, union_end) = union_line_span(existing_span, incoming_span);
    let union_limit = union_end.saturating_sub(union_start).saturating_add(1);

    let raw = if let Some(disk) = disk_full {
        extract_lines_1_indexed(disk, union_start, union_limit)
    } else if let Some(pre) = premerged_raw {
        pre.to_string()
    } else {
        return incoming;
    };

    ReadCacheEntry {
        mtime_secs: incoming.mtime_secs,
        raw_content: raw,
        offset: Some(union_start),
        limit: Some(union_limit),
        total_lines: incoming.total_lines,
        source: incoming.source,
        transcript_committed: existing.transcript_committed,
        committed_spans: existing.committed_spans.clone(),
        committed_offset: existing.committed_offset,
        committed_limit: existing.committed_limit,
    }
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
    if !entry.transcript_committed || entry.committed_spans.is_empty() {
        return false;
    }
    spans.iter().all(|&(s, e)| {
        entry
            .committed_spans
            .iter()
            .any(|&(cs, ce)| s >= cs && e <= ce)
    })
}

fn match_span_within_committed(entry: &ReadCacheEntry, s: usize, e: usize) -> bool {
    if entry.is_full_read() {
        return entry.transcript_committed;
    }
    entry
        .committed_spans
        .iter()
        .any(|&(cs, ce)| s >= cs && e <= ce)
}

fn edit_span_violation_error(entry: &ReadCacheEntry, line: usize, rel_path: &str) -> ToolError {
    let max = crate::read_economy::max_lines_for_path(rel_path)
        .unwrap_or(crate::read_economy::CHAPTER_MAX_LINES);
    let (offset, limit) = suggest_read_window(line, entry.total_lines.max(line), max);
    let read_span = if entry.transcript_committed && !entry.committed_spans.is_empty() {
        entry.format_committed_spans()
    } else {
        entry
            .cached_line_span()
            .map(|(s, e)| format!("{s}–{e}"))
            .unwrap_or_else(|| "?".into())
    };
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

/// Edit guard: single replace — match line span must lie within committed span (R1).
/// `replace_all` with session cache skips R1; disk byte match is sufficient.
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

    if replace_all {
        return Ok(());
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
    let check = spans.first().map(std::slice::from_ref).unwrap_or(&[]);

    if spans_within_committed_span(check, entry) {
        return Ok(());
    }

    if spans_within_cached_span(check, entry) {
        let line = check
            .iter()
            .find(|&&(s, e)| !match_span_within_committed(entry, s, e))
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

/// After Edit: single replace patches partial slice; `replace_all` promotes to full file.
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

    if replace_all || entry.is_full_read() {
        entry.mtime_secs = mtime_secs;
        entry.raw_content = updated_disk.to_string();
        entry.offset = None;
        entry.limit = None;
        entry.total_lines = updated_disk.lines().count();
        entry.source = ReadCacheSource::EditPatched;
        return;
    }

    let old_lines = old_string.lines().count().max(1);
    let new_lines = new_string.lines().count().max(1);
    let delta = (new_lines as i64 - old_lines as i64) * occurrences_replaced as i64;

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
        let first_edit_line = line_number_1_indexed(
            updated_disk,
            new_string.lines().next().unwrap_or(new_string),
        )
        .unwrap_or(offset);
        shift_committed_spans_for_edit(&mut entry.committed_spans, first_edit_line, delta);
        sync_committed_bounding_fields(entry);
    }
    entry.mtime_secs = mtime_secs;
    entry.source = ReadCacheSource::EditPatched;
}

pub fn is_read_dedup_stub(content: &str) -> bool {
    content.contains("File has not been changed since last read")
        && content.contains("Refer to the earlier Read")
}

fn format_read_dedup_hint(tool_name: &str, file_path: &str, range: &str) -> String {
    format!(
        "[read-dedup] path={file_path} range={range}\n\
         Disk unchanged; identical {tool_name} already in this conversation — do NOT retry same parameters.\n\
         Use earlier tool_result. Partial Read windows merge on repeat Read/Tail (same mtime); each Read/Tail output still ≤ read-economy max.\n\
         After partial Edit: cache is patched but conversation may be stale — use next Edit with updated text, or Read with different offset/limit (not same params). replace_all promotes cache to full file.\n\
         Long chapters: Grep → Read range / Tail; full Read only when ≤200 lines."
    )
}

pub fn format_read_dedup_hint_from_input(
    registry: &crate::ToolRegistry,
    tool_name: &str,
    input: &Value,
) -> Option<String> {
    let tool = registry.get(tool_name)?;
    let range = tool.read_dedup_range_label(input)?;
    let path = normalize_rel_path(&optional_file_path(input)?);
    Some(format_read_dedup_hint(tool_name, &path, &range))
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
            committed_spans: Vec::new(),
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
            committed_spans: Vec::new(),
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
            committed_spans: Vec::new(),
            committed_offset: None,
            committed_limit: None,
        };
        let merged = merge_read_cache_on_store(Some(&existing), incoming, Some(&disk), None);
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
            committed_spans: Vec::new(),
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
            committed_spans: Vec::new(),
            committed_offset: None,
            committed_limit: None,
        };
        let merged =
            merge_read_cache_on_store(Some(&existing), incoming.clone(), Some(&disk), None);
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
    fn replace_all_ok_when_match_outside_cached_span() {
        let disk = "alpha\nbeta\nalpha\n".to_string();
        let entry = partial_entry(&disk, 2, 1, ReadCacheSource::Read);
        assert!(verify_edit_against_read_cache(
            Some(&entry),
            "alpha",
            &disk,
            "chapters/ch01.md",
            true
        )
        .is_ok());
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
            committed_spans: Vec::new(),
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
    fn patch_replace_all_promotes_to_full_file() {
        let mut lines: Vec<String> = (1..=50).map(|n| format!("line {n}")).collect();
        lines[34] = "TAG mid".into();
        lines[39] = "TAG end".into();
        let disk = lines.join("\n");
        let mut entry = partial_entry(&disk, 33, 11, ReadCacheSource::Read);
        let old = "TAG";
        let new = "TAG\nextra";
        let updated = disk.replace(old, new);
        patch_read_cache_after_edit(&mut entry, &updated, 2, old, new, true, 2);
        assert!(entry.is_full_read());
        assert_eq!(entry.raw_content, updated);
        assert_eq!(entry.total_lines, 52);
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
            committed_spans: Vec::new(),
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
            committed_spans: Vec::new(),
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
        let h = format_read_dedup_hint("Read", "chapters/ch01.md", "offset:5 limit:8");
        assert!(h.contains("path=chapters/ch01.md"));
        assert!(h.contains("offset:5 limit:8"));
    }

    #[test]
    fn format_read_dedup_hint_tail() {
        let h = format_read_dedup_hint("Tail", "chapters/ch01.md", "tail:last_100_lines");
        assert!(h.contains("tail:last_100_lines"));
    }

    #[test]
    fn format_read_dedup_hint_from_input_via_registry() {
        let registry = crate::default_registry();
        let input = serde_json::json!({"file_path": "chapters/ch01.md", "offset": 3, "limit": 10});
        let hint = format_read_dedup_hint_from_input(&registry, "Read", &input).unwrap();
        assert!(hint.contains("offset:3 limit:10"));
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

    #[test]
    fn merge_allows_disjoint_reads_when_each_within_economy() {
        let disk = lines_disk(116);
        let mut existing = partial_entry(&disk, 1, 80, ReadCacheSource::Read);
        existing.mtime_secs = 5;
        let incoming = ReadCacheEntry {
            mtime_secs: 5,
            raw_content: extract_lines_1_indexed(&disk, 81, 36),
            offset: Some(81),
            limit: Some(36),
            total_lines: 116,
            source: ReadCacheSource::Read,
            transcript_committed: false,
            committed_spans: Vec::new(),
            committed_offset: None,
            committed_limit: None,
        };
        let merged = merge_read_cache_on_store(Some(&existing), incoming, Some(&disk), None);
        assert_eq!(merged.offset, Some(1));
        assert_eq!(merged.limit, Some(116));
        assert!(merged.raw_content.contains("line 81"));
    }

    #[test]
    fn gap_committed_does_not_cover_unread_hole() {
        let disk = lines_disk(30);
        let mut entry = ReadCacheEntry {
            mtime_secs: 1,
            raw_content: extract_lines_1_indexed(&disk, 10, 15),
            offset: Some(10),
            limit: Some(15),
            total_lines: 30,
            source: ReadCacheSource::Read,
            transcript_committed: true,
            committed_spans: Vec::new(),
            committed_offset: None,
            committed_limit: None,
        };
        entry.commit_span((10, 14));
        entry.commit_span((20, 24));
        assert_eq!(entry.committed_spans, vec![(10, 14), (20, 24)]);
        assert!(verify_edit_against_read_cache(
            Some(&entry),
            "line 22",
            &disk,
            "chapters/ch01.md",
            false,
        )
        .is_ok());
        let err = verify_edit_against_read_cache(
            Some(&entry),
            "line 17",
            &disk,
            "chapters/ch01.md",
            false,
        )
        .unwrap_err()
        .to_string();
        assert!(
            err.contains("not yet in the conversation") || err.contains("only read a portion"),
            "{err}"
        );
    }

    #[test]
    fn adjacent_committed_spans_coalesce() {
        let disk = lines_disk(116);
        let mut entry = ReadCacheEntry {
            mtime_secs: 1,
            raw_content: extract_lines_1_indexed(&disk, 1, 116),
            offset: Some(1),
            limit: Some(116),
            total_lines: 116,
            source: ReadCacheSource::Read,
            transcript_committed: false,
            committed_spans: Vec::new(),
            committed_offset: None,
            committed_limit: None,
        };
        entry.commit_span((1, 80));
        entry.commit_span((81, 116));
        assert_eq!(entry.committed_spans, vec![(1, 116)]);
        assert!(verify_edit_against_read_cache(
            Some(&entry),
            "line 90",
            &disk,
            "knowledge/plot/x.md",
            false,
        )
        .is_ok());
    }

    #[test]
    fn span_from_tool_input_matches_read_tail() {
        let registry = crate::default_registry();
        let read = span_from_tool_input(
            &registry,
            "Read",
            &serde_json::json!({"offset": 10, "limit": 5}),
            30,
        )
        .unwrap();
        assert_eq!(read, (10, 14));
        let tail =
            span_from_tool_input(&registry, "Tail", &serde_json::json!({"lines": 10}), 30).unwrap();
        assert_eq!(tail, (21, 30));
    }

    // -- shift_committed_spans_for_edit tests --

    #[test]
    fn shift_spans_edit_above_all_spans_shifts_them_down() {
        let mut spans = vec![(50, 69)];
        shift_committed_spans_for_edit(&mut spans, 10, 5);
        assert_eq!(spans, vec![(55, 74)]);
    }

    #[test]
    fn shift_spans_edit_below_all_spans_leaves_them_unchanged() {
        let mut spans = vec![(10, 20)];
        shift_committed_spans_for_edit(&mut spans, 80, 5);
        assert_eq!(spans, vec![(10, 20)]);
    }

    #[test]
    fn shift_spans_edit_before_span_shifts_span_forward() {
        let mut spans = vec![(10, 20)];
        shift_committed_spans_for_edit(&mut spans, 5, 3);
        assert_eq!(spans, vec![(13, 23)]);
    }

    #[test]
    fn shift_spans_edit_straddles_span_start_shifts_only_end() {
        let mut spans = vec![(10, 20)];
        shift_committed_spans_for_edit(&mut spans, 15, 2);
        assert_eq!(spans, vec![(10, 22)]);
    }

    #[test]
    fn shift_spans_negative_delta_contracts_spans() {
        let mut spans = vec![(10, 30)];
        shift_committed_spans_for_edit(&mut spans, 15, -3);
        assert_eq!(spans, vec![(10, 27)]);
    }

    #[test]
    fn shift_spans_multiple_edits_accumulate() {
        let mut spans = vec![(10, 20)];
        shift_committed_spans_for_edit(&mut spans, 8, 5);
        assert_eq!(spans, vec![(15, 25)]);
        shift_committed_spans_for_edit(&mut spans, 5, 2);
        assert_eq!(spans, vec![(17, 27)]);
    }

    #[test]
    fn shift_spans_zero_delta_is_noop() {
        let mut spans = vec![(10, 20)];
        shift_committed_spans_for_edit(&mut spans, 1, 0);
        assert_eq!(spans, vec![(10, 20)]);
    }

    #[test]
    fn shift_spans_empty_list_is_noop() {
        let mut spans = vec![];
        shift_committed_spans_for_edit(&mut spans, 1, 10);
        assert!(spans.is_empty());
    }

    // -- merge_read_cache_on_store edge tests --

    #[test]
    fn merge_existing_none_returns_incoming() {
        let disk = lines_disk(10);
        let incoming = ReadCacheEntry {
            mtime_secs: 1,
            raw_content: "line 5".into(),
            offset: Some(5),
            limit: Some(1),
            total_lines: 10,
            source: ReadCacheSource::Read,
            transcript_committed: false,
            committed_spans: vec![],
            committed_offset: None,
            committed_limit: None,
        };
        let merged = merge_read_cache_on_store(None, incoming.clone(), Some(&disk), None);
        assert_eq!(merged.offset, incoming.offset);
    }

    #[test]
    fn merge_premerged_raw_path() {
        let disk = lines_disk(20);
        let existing = partial_entry(&disk, 5, 5, ReadCacheSource::Read);
        let incoming = ReadCacheEntry {
            mtime_secs: 1,
            raw_content: "pre-merged".into(),
            offset: Some(10),
            limit: Some(6),
            total_lines: 20,
            source: ReadCacheSource::Read,
            transcript_committed: false,
            committed_spans: vec![],
            committed_offset: None,
            committed_limit: None,
        };
        let merged =
            merge_read_cache_on_store(Some(&existing), incoming, None, Some("merged raw content"));
        assert_eq!(merged.raw_content, "merged raw content");
        assert_eq!(merged.offset, Some(5));
        assert_eq!(merged.limit, Some(11));
    }

    // -- verify_edit_against_read_cache edge tests --

    #[test]
    fn verify_edit_none_entry_returns_ok() {
        let disk = lines_disk(10);
        assert!(verify_edit_against_read_cache(None, "line 5", &disk, "test.md", false).is_ok());
    }

    #[test]
    fn verify_edit_full_read_not_committed_returns_pending() {
        let disk = lines_disk(10);
        let entry = ReadCacheEntry {
            mtime_secs: 1,
            raw_content: disk.clone(),
            offset: None,
            limit: None,
            total_lines: 10,
            source: ReadCacheSource::Read,
            transcript_committed: false,
            committed_spans: vec![],
            committed_offset: None,
            committed_limit: None,
        };
        let err = verify_edit_against_read_cache(Some(&entry), "line 5", &disk, "test.md", false)
            .unwrap_err()
            .to_string();
        assert!(err.contains("was not applied"));
        assert!(err.contains("not yet in the conversation"));
    }

    #[test]
    fn verify_edit_full_read_committed_returns_ok() {
        let disk = lines_disk(10);
        let mut entry = ReadCacheEntry {
            mtime_secs: 1,
            raw_content: disk.clone(),
            offset: None,
            limit: None,
            total_lines: 10,
            source: ReadCacheSource::Read,
            transcript_committed: false,
            committed_spans: vec![],
            committed_offset: None,
            committed_limit: None,
        };
        entry.commit_to_transcript();
        assert!(
            verify_edit_against_read_cache(Some(&entry), "line 5", &disk, "test.md", false).is_ok()
        );
    }
}
