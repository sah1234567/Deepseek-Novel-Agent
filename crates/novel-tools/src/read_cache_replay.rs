//! Rebuild session read cache by replaying Read/Tail/Write/Edit tool pairs from transcript.
//!
//! Compaction replay uses `messages_replay_cutoff` — partial merge unions from Reads before the
//! cutoff are not reconstructed; only tool pairs in the retained slice are replayed from disk.

use crate::paths::optional_file_path;
use crate::read_cache::{file_mtime_secs, is_read_dedup_stub};
use crate::read_cache_ingest::{ingest_read_or_tail_into_cache, is_tool_result_error};
use crate::{EditCachePatch, ReadCacheEntry, ToolContext, ToolError, ToolRegistry};
use dashmap::DashMap;
use serde_json::Value;
use std::path::{Path, PathBuf};
use std::sync::Arc;

/// One tool_use ↔ tool_result pair for cache rebuild (decoupled from `novel-core::ChatMessage`).
#[derive(Debug, Clone)]
pub struct ReadCacheReplayPair {
    pub tool_name: String,
    pub arguments: Value,
    pub result_content: String,
}

#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub(crate) struct RebuildStats {
    pub pairs_seen: usize,
    pub paths_cached: usize,
    pub skipped_error: usize,
    pub skipped_dedup: usize,
    pub disk_failures: usize,
}

/// Why a Read/Tail pair was not replayed into cache.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ReadTailReplaySkip {
    ErrorResult,
    DedupStub,
}

/// Replay Write/Edit: read current disk + mtime, then refresh or patch cache (same rules as live tools).
pub(crate) fn replay_write_or_edit_from_disk(
    ctx: &ToolContext,
    tool_name: &str,
    full: &Path,
    input: &Value,
) -> Result<(), ToolError> {
    let disk = std::fs::read_to_string(full).map_err(ToolError::Io)?;
    let mtime = std::fs::metadata(full)
        .ok()
        .map(|m| file_mtime_secs(&m))
        .unwrap_or(0);
    match tool_name {
        "Write" => {
            ctx.refresh_cache_after_write(full, &disk, mtime);
            Ok(())
        }
        "Edit" => {
            let old_string = input
                .get("old_string")
                .and_then(|v| v.as_str())
                .unwrap_or("");
            let new_string = input
                .get("new_string")
                .and_then(|v| v.as_str())
                .unwrap_or("");
            let replace_all = input
                .get("replace_all")
                .and_then(|v| v.as_bool())
                .unwrap_or(false);
            if replace_all || ctx.read_cache_entry(&full.to_path_buf()).is_some() {
                let occ = if replace_all {
                    disk.matches(old_string).count().max(1)
                } else {
                    1
                };
                ctx.patch_cache_after_edit(
                    full,
                    &EditCachePatch {
                        updated_disk: &disk,
                        mtime_secs: mtime,
                        old_string,
                        new_string,
                        replace_all,
                        occurrences_replaced: occ,
                    },
                );
            } else {
                ctx.refresh_cache_after_write(full, &disk, mtime);
            }
            Ok(())
        }
        _ => Ok(()),
    }
}

/// Replay Read/Tail: disk ingest (merge union when same mtime) + promote committed span from tool input.
fn replay_read_or_tail_from_disk(
    ctx: &ToolContext,
    registry: &ToolRegistry,
    tool_name: &str,
    full: &Path,
    input: &Value,
    result_content: &str,
) -> Result<(), ReadTailReplaySkip> {
    if is_tool_result_error(result_content) {
        return Err(ReadTailReplaySkip::ErrorResult);
    }
    if is_read_dedup_stub(result_content) {
        return Err(ReadTailReplaySkip::DedupStub);
    }
    let source = if tool_name == "Read" {
        crate::ReadCacheSource::Read
    } else {
        crate::ReadCacheSource::Tail
    };
    ingest_read_or_tail_into_cache(ctx, tool_name, full, input, source, None)
        .map_err(|_| ReadTailReplaySkip::ErrorResult)?;
    ctx.promote_read_cache_committed(registry, full, tool_name, input);
    Ok(())
}

pub fn rebuild_read_cache_from_pairs(
    cache: &Arc<DashMap<PathBuf, ReadCacheEntry>>,
    project_root: &Path,
    registry: &ToolRegistry,
    pairs: &[ReadCacheReplayPair],
) {
    let stats = rebuild_read_cache_from_pairs_inner(cache, project_root, registry, pairs);
    tracing::debug!(
        pairs_seen = stats.pairs_seen,
        paths_cached = stats.paths_cached,
        skipped_error = stats.skipped_error,
        skipped_dedup = stats.skipped_dedup,
        disk_failures = stats.disk_failures,
        "read_cache_rebuilt"
    );
}

fn rebuild_read_cache_from_pairs_inner(
    cache: &Arc<DashMap<PathBuf, ReadCacheEntry>>,
    project_root: &Path,
    registry: &ToolRegistry,
    pairs: &[ReadCacheReplayPair],
) -> RebuildStats {
    let mut stats = RebuildStats::default();
    cache.clear();

    let ctx = ToolContext::for_cache_rebuild(project_root.to_path_buf(), Arc::clone(cache));

    for pair in pairs {
        stats.pairs_seen += 1;
        let name = pair.tool_name.as_str();
        let input = &pair.arguments;
        let content = &pair.result_content;

        match name {
            "Read" | "Tail" => {
                let Some(rel) = optional_file_path(input) else {
                    continue;
                };
                let full = ctx.resolve_path(&rel);
                match replay_read_or_tail_from_disk(&ctx, registry, name, &full, input, content) {
                    Ok(()) => {}
                    Err(ReadTailReplaySkip::ErrorResult) => {
                        stats.disk_failures += 1;
                        tracing::warn!(path = %rel, tool = %name, "read_cache_rebuild disk ingest failed");
                    }
                    Err(ReadTailReplaySkip::DedupStub) => {
                        stats.skipped_dedup += 1;
                    }
                }
            }
            "Write" | "Edit" => {
                let Some(rel) = optional_file_path(input) else {
                    continue;
                };
                let full = ctx.resolve_path(&rel);
                if let Err(e) = replay_write_or_edit_from_disk(&ctx, name, &full, input) {
                    stats.disk_failures += 1;
                    tracing::warn!(path = %rel, tool = %name, error = %e, "read_cache_rebuild write/edit failed");
                }
            }
            _ => {}
        }
    }

    stats.paths_cached = cache.len();
    stats
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    fn lines(n: usize) -> String {
        (1..=n)
            .map(|i| format!("line {i}"))
            .collect::<Vec<_>>()
            .join("\n")
    }

    #[test]
    fn rebuild_read_partial_commits_span() {
        let tmp = TempDir::new().unwrap();
        let path = tmp.path().join("ch.md");
        std::fs::write(&path, lines(50)).unwrap();

        let registry = crate::default_registry();
        let cache = Arc::new(DashMap::new());
        let pairs = vec![ReadCacheReplayPair {
            tool_name: "Read".into(),
            arguments: serde_json::json!({
                "file_path": "ch.md",
                "offset": 33,
                "limit": 11
            }),
            result_content: "33\tline 33".into(),
        }];
        let stats = rebuild_read_cache_from_pairs_inner(&cache, tmp.path(), &registry, &pairs);
        assert_eq!(stats.paths_cached, 1);
        let entry = cache.get(&path).unwrap();
        assert!(entry.transcript_committed);
        assert_eq!(entry.committed_spans, vec![(33, 43)]);
    }

    #[test]
    fn replay_write_refreshes_cache_from_disk() {
        let tmp = TempDir::new().unwrap();
        let path = tmp.path().join("note.md");
        std::fs::write(&path, "hello\nworld\n").unwrap();
        let cache = Arc::new(DashMap::new());
        let ctx = ToolContext::for_cache_rebuild(tmp.path().to_path_buf(), Arc::clone(&cache));

        replay_write_or_edit_from_disk(&ctx, "Write", &path, &serde_json::json!({})).unwrap();

        let entry = cache.get(&path).unwrap();
        assert_eq!(entry.raw_content, "hello\nworld\n");
        assert!(entry.transcript_committed);
    }

    #[test]
    fn replay_edit_without_cache_refreshes_from_disk() {
        let tmp = TempDir::new().unwrap();
        let path = tmp.path().join("note.md");
        std::fs::write(&path, "alpha\nbeta\n").unwrap();
        let cache = Arc::new(DashMap::new());
        let ctx = ToolContext::for_cache_rebuild(tmp.path().to_path_buf(), Arc::clone(&cache));
        let input = serde_json::json!({
            "old_string": "alpha",
            "new_string": "ALPHA"
        });

        replay_write_or_edit_from_disk(&ctx, "Edit", &path, &input).unwrap();

        let entry = cache.get(&path).unwrap();
        assert_eq!(entry.raw_content, "alpha\nbeta\n");
    }

    #[test]
    fn replay_edit_with_cache_patches_slice() {
        let tmp = TempDir::new().unwrap();
        let path = tmp.path().join("ch.md");
        std::fs::write(&path, lines(50)).unwrap();
        let cache = Arc::new(DashMap::new());
        let registry = crate::default_registry();
        let read_pairs = vec![ReadCacheReplayPair {
            tool_name: "Read".into(),
            arguments: serde_json::json!({
                "file_path": "ch.md",
                "offset": 10,
                "limit": 5
            }),
            result_content: "10\tline 10".into(),
        }];
        rebuild_read_cache_from_pairs_inner(&cache, tmp.path(), &registry, &read_pairs);
        std::fs::write(&path, lines(50).replacen("line 10", "LINE 10", 1)).unwrap();

        let ctx = ToolContext::for_cache_rebuild(tmp.path().to_path_buf(), Arc::clone(&cache));
        let input = serde_json::json!({
            "file_path": "ch.md",
            "old_string": "line 10",
            "new_string": "LINE 10"
        });
        replay_write_or_edit_from_disk(&ctx, "Edit", &path, &input).unwrap();

        let entry = cache.get(&path).unwrap();
        assert!(entry.raw_content.contains("LINE 10"));
        assert_eq!(entry.offset, Some(10));
    }

    #[test]
    fn replay_edit_replace_all_patches_from_disk() {
        let tmp = TempDir::new().unwrap();
        let path = tmp.path().join("tags.md");
        std::fs::write(&path, "bar bar\n").unwrap();
        let cache = Arc::new(DashMap::new());
        let ctx = ToolContext::for_cache_rebuild(tmp.path().to_path_buf(), Arc::clone(&cache));
        ctx.refresh_cache_after_write(&path, "foo foo\n", 0);
        let input = serde_json::json!({
            "old_string": "foo",
            "new_string": "bar",
            "replace_all": true
        });

        replay_write_or_edit_from_disk(&ctx, "Edit", &path, &input).unwrap();

        let entry = cache.get(&path).unwrap();
        assert_eq!(entry.raw_content, "bar bar\n");
    }

    #[test]
    fn rebuild_overlapping_partial_reads_union_merge() {
        let tmp = TempDir::new().unwrap();
        let path = tmp.path().join("ch.md");
        std::fs::write(&path, lines(50)).unwrap();

        let registry = crate::default_registry();
        let cache = Arc::new(DashMap::new());
        let pairs = vec![
            ReadCacheReplayPair {
                tool_name: "Read".into(),
                arguments: serde_json::json!({
                    "file_path": "ch.md",
                    "offset": 10,
                    "limit": 5
                }),
                result_content: "10\tline 10".into(),
            },
            ReadCacheReplayPair {
                tool_name: "Read".into(),
                arguments: serde_json::json!({
                    "file_path": "ch.md",
                    "offset": 12,
                    "limit": 5
                }),
                result_content: "12\tline 12".into(),
            },
        ];
        let stats = rebuild_read_cache_from_pairs_inner(&cache, tmp.path(), &registry, &pairs);
        assert_eq!(stats.paths_cached, 1);
        let entry = cache.get(&path).unwrap();
        assert_eq!(entry.offset, Some(10));
        assert_eq!(entry.limit, Some(7));
        assert_eq!(entry.committed_spans, vec![(10, 16)]);
    }
}
