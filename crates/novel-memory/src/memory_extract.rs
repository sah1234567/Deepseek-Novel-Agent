//! Memory extraction: background fire-and-forget subagent that scans recent
//! conversation messages and creates/updates memory files.

use crate::memory_extract_prompt::build_memory_extraction_prompt;
use crate::memory_scan::{format_memory_manifest, scan_memory_files_for_extraction};
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::sync::Mutex;

/// Context passed to the memory extractor for each turn.
#[derive(Debug, Clone)]
pub struct ExtractionContext {
    /// All messages in the current session (for cursor tracking)
    pub message_count: usize,
    /// Project root path
    pub project_root: PathBuf,
    /// True when the main agent already Write/Edit'd a file under `memory/` this turn.
    pub main_agent_wrote_memory: bool,
    /// True when AskUserQuestion was called during this turn.  Overrides
    /// `main_agent_wrote_memory` — user decisions must always be extracted.
    pub had_ask_user_question: bool,
}

/// Prepared extraction job — pass to `novel-core` to spawn the fork subagent.
#[derive(Debug, Clone)]
pub struct PreparedMemoryExtraction {
    pub task_prompt: String,
    pub message_count: usize,
}

/// Memory extractor state machine.
pub struct MemoryExtractor {
    last_processed_message_count: Mutex<usize>,
    in_progress: AtomicBool,
    pending_context: Mutex<Option<ExtractionContext>>,
    /// Counts eligible turns since the last extraction.  When ≥ throttle,
    /// the next eligible turn triggers extraction; then the counter resets.
    /// Throttle is [`crate::memory_types::MemoryConstants::EXTRACTION_THROTTLE_TURNS`].
    turns_since_last_extraction: AtomicU32,
}

impl MemoryExtractor {
    pub fn new() -> Self {
        Self {
            last_processed_message_count: Mutex::new(0),
            in_progress: AtomicBool::new(false),
            pending_context: Mutex::new(None),
            turns_since_last_extraction: AtomicU32::new(0),
        }
    }

    pub fn cursor(&self) -> usize {
        *self
            .last_processed_message_count
            .lock()
            .unwrap_or_else(|e| e.into_inner())
    }

    pub fn advance_cursor_to(&self, count: usize) {
        let mut cursor = self
            .last_processed_message_count
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        *cursor = count;
    }

    pub fn should_run(&self, ctx: &ExtractionContext) -> bool {
        if self.in_progress.load(Ordering::Acquire) {
            return false;
        }
        ctx.message_count > self.cursor()
    }

    /// Try to begin an extraction. Returns `Some(job)` when the fork should run.
    ///
    /// Gating order (each stage short-circuits on rejection):
    /// 1. Main-agent memory write → skip (avoid duplicate extraction)
    /// 2. Throttle gate → skip if below period
    /// 3. In-progress gate → coalesce pending if new messages exist
    /// 4. No-new-messages gate → skip
    /// 5. CAS acquire → build prompt
    pub fn try_prepare_extraction(
        &self,
        ctx: &ExtractionContext,
    ) -> Option<PreparedMemoryExtraction> {
        // 1. Main agent already handled memory this turn — skip unless
        //    AskUserQuestion was called (user decisions must always be reviewed).
        if ctx.main_agent_wrote_memory && !ctx.had_ask_user_question {
            self.advance_cursor_to(ctx.message_count);
            return None;
        }

        // 2. Throttle gate — skipped when `had_ask_user_question` (user decisions must run).
        if !ctx.had_ask_user_question && !self.pass_throttle_gate() {
            return None;
        }

        // 3. Already running a previous extraction?
        if self.in_progress.load(Ordering::Acquire) {
            self.coalesce_pending(ctx);
            return None;
        }

        // 4. No new messages to extract from.
        if ctx.message_count <= self.cursor() {
            return None;
        }

        // 5. CAS acquire — another thread may have raced us.
        if self.in_progress.swap(true, Ordering::AcqRel) {
            self.coalesce_pending(ctx);
            return None;
        }

        self.turns_since_last_extraction.store(0, Ordering::Relaxed);
        let task_prompt = self.build_task_prompt(ctx);
        Some(PreparedMemoryExtraction {
            task_prompt,
            message_count: ctx.message_count,
        })
    }

    /// Returns `true` when the throttle gate allows extraction.
    ///
    /// With `EXTRACTION_THROTTLE_TURNS = 1` (default), every eligible turn passes.
    /// With higher values, extraction runs once every N turns.
    fn pass_throttle_gate(&self) -> bool {
        let throttle = crate::memory_types::MemoryConstants::EXTRACTION_THROTTLE_TURNS;
        if throttle <= 1 {
            return true;
        }
        // fetch_add returns the previous value; the sequence with throttle=2:
        // turn 0→1 (skip), turn 1→2 (run, reset to 0), turn 0→1 (skip), …
        let prev = self
            .turns_since_last_extraction
            .fetch_add(1, Ordering::Relaxed);
        prev + 1 >= throttle
    }

    /// Stash the current context for a trailing extraction run.
    fn coalesce_pending(&self, ctx: &ExtractionContext) {
        if ctx.message_count > self.cursor() {
            let mut pending = self
                .pending_context
                .lock()
                .unwrap_or_else(|e| e.into_inner());
            *pending = Some(ctx.clone());
        }
    }

    /// Mark extraction complete and run any coalesced trailing job.
    pub fn complete_extraction(&self, message_count: usize) -> Option<PreparedMemoryExtraction> {
        self.advance_cursor_to(message_count);

        let trailing = self
            .pending_context
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .take();

        self.in_progress.store(false, Ordering::Release);

        trailing.map(|ctx| {
            self.in_progress.store(true, Ordering::Release);
            PreparedMemoryExtraction {
                task_prompt: self.build_task_prompt(&ctx),
                message_count: ctx.message_count,
            }
        })
    }

    pub fn build_task_prompt(&self, ctx: &ExtractionContext) -> String {
        let memory_dir = ctx.project_root.join("memory");
        let headers = scan_memory_files_for_extraction(&memory_dir);
        let manifest = format_memory_manifest(&headers, true);
        let new_count = ctx.message_count.saturating_sub(self.cursor());
        // Ask-user priority is in `prompt/memory/extraction-task.md` (static template).
        build_memory_extraction_prompt(new_count, &manifest, "memory")
    }

    pub fn reset(&self) {
        self.in_progress.store(false, Ordering::Release);
        self.turns_since_last_extraction.store(0, Ordering::Relaxed);
        let mut pending = self
            .pending_context
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        *pending = None;
        let mut cursor = self
            .last_processed_message_count
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        *cursor = 0;
    }
}

impl Default for MemoryExtractor {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    fn ctx(message_count: usize, project_root: PathBuf) -> ExtractionContext {
        ExtractionContext {
            message_count,
            project_root,
            main_agent_wrote_memory: false,
            had_ask_user_question: false,
        }
    }

    #[test]
    fn prepare_runs_when_main_agent_wrote_memory_but_ask_user_question_was_called() {
        let extractor = MemoryExtractor::new();
        let tmp = TempDir::new().unwrap();
        let ctx = ExtractionContext {
            message_count: 5,
            project_root: tmp.path().to_path_buf(),
            main_agent_wrote_memory: true,
            had_ask_user_question: true,
        };
        assert!(extractor.try_prepare_extraction(&ctx).is_some());
    }

    #[test]
    fn extractor_starts_with_zero_cursor() {
        let extractor = MemoryExtractor::new();
        assert_eq!(extractor.cursor(), 0);
    }

    #[test]
    fn should_run_when_new_messages_exist() {
        let extractor = MemoryExtractor::new();
        assert!(extractor.should_run(&ctx(10, PathBuf::from("/tmp"))));
    }

    #[test]
    fn should_not_run_when_no_new_messages() {
        let extractor = MemoryExtractor::new();
        extractor.advance_cursor_to(10);
        assert!(!extractor.should_run(&ctx(10, PathBuf::from("/tmp"))));
    }

    #[test]
    fn should_not_run_when_in_progress() {
        let extractor = MemoryExtractor::new();
        extractor.in_progress.store(true, Ordering::Release);
        assert!(!extractor.should_run(&ctx(10, PathBuf::from("/tmp"))));
    }

    #[test]
    fn prepare_advances_cursor_after_complete() {
        let extractor = MemoryExtractor::new();
        let tmp = TempDir::new().unwrap();
        let prepared = extractor
            .try_prepare_extraction(&ctx(5, tmp.path().to_path_buf()))
            .expect("prepared");
        assert_eq!(prepared.message_count, 5);
        extractor.complete_extraction(prepared.message_count);
        assert_eq!(extractor.cursor(), 5);
    }

    #[test]
    fn prepare_skips_when_main_agent_already_wrote_memory() {
        let extractor = MemoryExtractor::new();
        let tmp = TempDir::new().unwrap();
        let mut extraction_ctx = ctx(5, tmp.path().to_path_buf());
        extraction_ctx.main_agent_wrote_memory = true;
        assert!(extractor.try_prepare_extraction(&extraction_ctx).is_none());
        assert_eq!(extractor.cursor(), 5);
    }

    #[test]
    fn coalesced_extraction_runs_trailing_context() {
        let extractor = MemoryExtractor::new();
        let tmp = TempDir::new().unwrap();
        let first = extractor
            .try_prepare_extraction(&ctx(5, tmp.path().to_path_buf()))
            .expect("first job");
        assert!(extractor
            .try_prepare_extraction(&ctx(8, tmp.path().to_path_buf()))
            .is_none());

        let trailing = extractor
            .complete_extraction(first.message_count)
            .expect("trailing job");
        assert_eq!(trailing.message_count, 8);
        assert!(extractor.in_progress.load(Ordering::Acquire));
    }

    #[test]
    fn reset_clears_state() {
        let extractor = MemoryExtractor::new();
        extractor.advance_cursor_to(10);
        extractor.in_progress.store(true, Ordering::Release);
        extractor.reset();
        assert_eq!(extractor.cursor(), 0);
        assert!(!extractor.in_progress.load(Ordering::Acquire));
    }

    #[test]
    fn build_task_prompt_works_with_empty_memory_dir() {
        let extractor = MemoryExtractor::new();
        let tmp = TempDir::new().unwrap();
        let prompt = extractor.build_task_prompt(&ctx(5, tmp.path().to_path_buf()));
        assert!(prompt.contains("尚无 memory 文件") || prompt.contains("5"));
        assert!(prompt.contains("什么都不做") || prompt.contains("没有"));
    }
}
