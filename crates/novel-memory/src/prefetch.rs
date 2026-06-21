//! Memory prefetch: background memory relevance selection that runs in
//! parallel with the main model's streaming response.
//!
//! Integrated into `AgentEngine::handle_message_with_events`:
//! - **Start**: after `init_llm()`, spawn background scan + Flash sideQuery
//! - **Consume**: after `run_inner_turn_loop()`, inject surfaced memories
//!   as a user message so the LLM sees them on the next turn.
//!
//! ## Dedup strategy
//!
//! Before selection, `collect_surfaced_paths()` scans existing messages for
//! memory attachments injected in previous turns. These paths are excluded
//! from the candidate list so the Flash selector never sees them. After
//! compaction clears old messages, all paths become eligible again.

use std::collections::HashSet;
use std::path::{Path, PathBuf};

use crate::memory_scan::scan_memory_files;
use crate::memory_select::read_memories_for_surfacing;
use crate::memory_types::SurfacedMemory;

use crate::selection::{select_relevant, MemorySelector};

/// Handle for an in-flight memory prefetch.
///
/// Created at turn start; consumed at turn end.
pub struct MemoryPrefetch {
    handle: Option<tokio::task::JoinHandle<Vec<SurfacedMemory>>>,
}

impl MemoryPrefetch {
    /// Start a memory prefetch in the background.
    ///
    /// `surfaced_paths` should be the set of memory file paths already
    /// injected in previous turns (from `collect_surfaced_paths`).
    /// These files are excluded from selection to avoid re-injection.
    ///
    /// If `selector` is `None` (offline / no API key), returns an empty
    /// prefetch that immediately yields no results.
    pub fn start<S: MemorySelector + 'static>(
        selector: Option<S>,
        query: String,
        memory_dir: PathBuf,
        surfaced_paths: HashSet<String>,
    ) -> Self {
        let handle = selector.map(|sel| {
            tokio::spawn(async move {
                run_prefetch_pipeline(sel, &query, &memory_dir, &surfaced_paths).await
            })
        });

        MemoryPrefetch { handle }
    }

    /// Consume the prefetch results, blocking until complete if still running.
    ///
    /// Returns surfaced memories ready for injection into the context.
    pub async fn consume(self) -> Vec<SurfacedMemory> {
        if let Some(handle) = self.handle {
            match handle.await {
                Ok(memories) => memories,
                Err(e) => {
                    tracing::debug!(
                        error = %e,
                        "memory_prefetch_join_error"
                    );
                    Vec::new()
                }
            }
        } else {
            Vec::new()
        }
    }

    /// Collect memory file paths that have already been surfaced in previous
    /// turns — these should not be re-selected.
    ///
    /// Scans message content strings for `Memory (记录于 ...): <path>:` patterns
    /// injected by previous prefetch consumptions.
    ///
    /// Decoupled from `novel-core::ChatMessage`: accepts raw message content
    /// strings rather than the full message struct.
    pub fn collect_surfaced_paths<'a>(
        message_contents: impl IntoIterator<Item = &'a str>,
    ) -> HashSet<String> {
        let mut paths = HashSet::new();
        for content in message_contents {
            for line in content.lines() {
                // Match: "Memory (记录于 ChN): style/pacing.md:"
                if let Some(rest) = line.strip_prefix("Memory (记录于 ") {
                    if let Some(after_chapter) = rest.split_once("): ").map(|x| x.1) {
                        if let Some(path) = after_chapter.strip_suffix(':') {
                            if path.ends_with(".md") {
                                paths.insert(path.to_string());
                            }
                        }
                    }
                }
            }
        }
        paths
    }

    /// Sum the on-disk bytes of already-surfaced memory files.
    ///
    /// Used to enforce [`MemoryConstants::MAX_SESSION_BYTES`] — once the
    /// session has surfaced enough memory, further prefetches are skipped.
    pub fn count_surfaced_bytes(surfaced_paths: &HashSet<String>, memory_dir: &Path) -> usize {
        surfaced_paths
            .iter()
            .map(|p| {
                memory_dir
                    .join(p)
                    .metadata()
                    .map(|m| m.len() as usize)
                    .unwrap_or(0)
            })
            .sum()
    }

    /// Format a single surfaced memory as an attachment string for injection
    /// into the LLM context as a user message.
    pub fn format_attachment(memory: &SurfacedMemory) -> String {
        let h = &memory.header;
        let header = format!("Memory (记录于 {}): {}:", h.frontmatter.chapter, h.rel_path);
        let truncated_note = if memory.truncated {
            "\n> [truncated]"
        } else {
            ""
        };
        format!(
            "{header}\n{memory}{truncated_note}",
            memory = memory.content
        )
    }
}

/// Full prefetch pipeline: scan → filter surfaced → select → read.
async fn run_prefetch_pipeline<S: MemorySelector>(
    mut selector: S,
    query: &str,
    memory_dir: &std::path::Path,
    surfaced_paths: &HashSet<String>,
) -> Vec<SurfacedMemory> {
    // 1. Scan all memory files
    let all_headers = scan_memory_files(memory_dir);

    if all_headers.is_empty() {
        tracing::debug!("memory_prefetch_no_files");
        return Vec::new();
    }

    // 2. Filter out already-surfaced files
    //    (deprecated files are already excluded by scan_memory_files)
    let candidates: Vec<_> = all_headers
        .iter()
        .filter(|h| !surfaced_paths.contains(&h.rel_path))
        .cloned()
        .collect();

    let filtered_count = all_headers.len() - candidates.len();
    if filtered_count > 0 {
        tracing::debug!(
            total = all_headers.len(),
            filtered = filtered_count,
            remaining = candidates.len(),
            "memory_prefetch_dedup"
        );
    }

    if candidates.is_empty() {
        return Vec::new();
    }

    // 3. Select relevant memories via Flash side query
    let selected = match select_relevant(&mut selector, query, &candidates).await {
        Ok(filenames) => filenames,
        Err(e) => {
            tracing::debug!(
                error = %e,
                "memory_prefetch_selection_failed"
            );
            return Vec::new();
        }
    };

    if selected.is_empty() {
        return Vec::new();
    }

    tracing::debug!(
        candidates = candidates.len(),
        selected = selected.len(),
        "memory_prefetch_complete"
    );

    // 4. Read full body of selected files
    read_memories_for_surfacing(memory_dir, &candidates, &selected)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::memory_types::{MemoryFrontmatter, MemoryHeader, MemoryStatus, MemoryType};

    #[test]
    fn collect_surfaced_paths_from_memory_attachment() {
        let contents = vec!["Memory (记录于 Ch5): style/pacing.md:\ncontent"];
        let paths = MemoryPrefetch::collect_surfaced_paths(contents);
        assert!(paths.contains("style/pacing.md"));
    }

    #[test]
    fn collect_surfaced_paths_ignores_non_memory_messages() {
        let contents = vec!["你好，请写第5章"];
        let paths = MemoryPrefetch::collect_surfaced_paths(contents);
        assert!(paths.is_empty());
    }

    #[test]
    fn format_attachment_includes_chapter_and_path() {
        let memory = SurfacedMemory {
            header: MemoryHeader {
                rel_path: "style/pacing.md".into(),
                memory_type: MemoryType::Style,
                frontmatter: MemoryFrontmatter {
                    name: "pacing".into(),
                    description: "d".into(),
                    chapter: "Ch5".into(),
                    status: MemoryStatus::Active,
                },
                mtime_ms: 0,
            },
            content: "每章结尾留悬念".into(),
            truncated: false,
        };
        let s = MemoryPrefetch::format_attachment(&memory);
        assert!(s.contains("Ch5"));
        assert!(s.contains("style/pacing.md"));
        assert!(s.contains("每章结尾留悬念"));
    }

    #[test]
    fn format_attachment_marks_truncated() {
        let memory = SurfacedMemory {
            header: MemoryHeader {
                rel_path: "style/long.md".into(),
                memory_type: MemoryType::Style,
                frontmatter: MemoryFrontmatter {
                    name: "long".into(),
                    description: "d".into(),
                    chapter: "Ch1".into(),
                    status: MemoryStatus::Active,
                },
                mtime_ms: 0,
            },
            content: "body".into(),
            truncated: true,
        };
        let s = MemoryPrefetch::format_attachment(&memory);
        assert!(s.contains("[truncated]"));
    }

    #[tokio::test]
    async fn prefetch_with_selector_runs_full_pipeline() {
        use crate::memory_types::SideQueryResult;
        use async_trait::async_trait;
        use novel_deepseek::LlmError;
        use std::fs;
        use tempfile::TempDir;

        let tmp = TempDir::new().unwrap();
        let mem_dir = tmp.path().join("memory");
        let style_dir = mem_dir.join("style");
        fs::create_dir_all(&style_dir).unwrap();
        let content = "---\nname: pacing\ndescription: d\nchapter: Ch1\nstatus: active\n---\n\n每章结尾留悬念";
        fs::write(style_dir.join("pacing.md"), content).unwrap();

        struct MockSelector {
            selected: Vec<String>,
        }
        #[async_trait]
        impl MemorySelector for MockSelector {
            async fn side_query(
                &mut self,
                _system: &str,
                _user_message: &str,
                _max_tokens: u32,
                _response_format: Option<serde_json::Value>,
            ) -> Result<SideQueryResult, LlmError> {
                Ok(SideQueryResult {
                    content: serde_json::json!({
                        "selected_memories": self.selected
                    })
                    .to_string(),
                })
            }
        }

        let prefetch = MemoryPrefetch::start(
            Some(MockSelector {
                selected: vec!["style/pacing.md".into()],
            }),
            "write chapter 5".into(),
            mem_dir,
            HashSet::new(),
        );
        let surfaced = prefetch.consume().await;
        assert_eq!(surfaced.len(), 1);
        assert!(surfaced[0].content.contains("每章结尾留悬念"));
        assert_eq!(surfaced[0].header.rel_path, "style/pacing.md");
    }

    #[tokio::test]
    async fn empty_prefetch_without_selector_returns_nothing() {
        let prefetch: MemoryPrefetch = MemoryPrefetch::start(
            None::<novel_deepseek::ChatClient>,
            "query".into(),
            PathBuf::from("/tmp"),
            HashSet::new(),
        );
        let results = prefetch.consume().await;
        assert!(results.is_empty());
    }
}
