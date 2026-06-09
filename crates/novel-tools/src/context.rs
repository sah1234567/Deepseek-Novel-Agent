use crate::paths::{normalize_rel_path, optional_file_path, resolve_under_project};
use crate::read_cache::{
    merge_read_cache_on_store, patch_read_cache_after_edit, read_range_key, span_from_tool_input,
    ReadCacheEntry, ReadCacheSource,
};
use crate::ToolError;
use std::future::Future;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PermissionMode {
    Normal,
    Plan,
    Auto,
    Unattended,
}

impl PermissionMode {
    /// Parse settings.json `permissions.mode`; unknown values default to `Normal`.
    pub fn from_settings_str(mode: &str) -> Self {
        match mode {
            "plan" => Self::Plan,
            "auto" => Self::Auto,
            "unattended" => Self::Unattended,
            _ => Self::Normal,
        }
    }

    /// Stable lowercase label for IPC / status snapshots.
    pub fn label(&self) -> &'static str {
        match self {
            Self::Normal => "normal",
            Self::Plan => "plan",
            Self::Auto => "auto",
            Self::Unattended => "unattended",
        }
    }

    /// Strict parse for IPC (`set_permission_mode`); unknown values are rejected.
    pub fn try_from_ipc(mode: &str) -> Result<Self, String> {
        match mode {
            "normal" => Ok(Self::Normal),
            "plan" => Ok(Self::Plan),
            "auto" => Ok(Self::Auto),
            "unattended" => Ok(Self::Unattended),
            other => Err(format!("invalid permission mode: {other}")),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PermissionResult {
    Allow,
    Deny { reason: String },
    Ask { tool_name: String, summary: String },
}

/// Pending subagent work: tool fork (`parent_tool_call_id` Some) or PostToolUse hook (None).
#[derive(Debug, Clone)]
pub struct PendingSubagentWork {
    pub agent_type: String,
    pub task: String,
    pub parent_tool_call_id: Option<String>,
}

/// Queued subagent jobs from `ForkSubAgent` tool and hook enqueue paths.
pub type SubagentWorkQueue = Arc<Mutex<Vec<PendingSubagentWork>>>;

#[derive(Clone)]
pub struct ToolContext {
    pub permission_mode: PermissionMode,
    pub deny_rules: Vec<String>,
    pub always_allow: Vec<String>,
    pub project_root: PathBuf,
    pub session_id: String,
    pub db: Option<Arc<novel_state::Database>>,
    /// Live permission override (UI permission mode selector).
    pub permission_mode_override: Option<Arc<Mutex<PermissionMode>>>,
    /// Paths read this session (canonical) for read-before-write enforcement and dedup.
    pub read_file_cache: Option<Arc<dashmap::DashMap<PathBuf, ReadCacheEntry>>>,
    /// Per-path serialization for Read/Tail/Edit/Write on the same file.
    pub file_op_locks: Option<Arc<dashmap::DashMap<PathBuf, Arc<tokio::sync::Mutex<()>>>>>,
    /// Main session only; false while any sub-agent inner loop runs (blocks nested fork).
    pub allow_fork: bool,
    /// Engine subagent work queue; present only on main-session tool context.
    pub subagent_queue: Option<SubagentWorkQueue>,
    /// Set by the executor for the in-flight tool call (ForkSubAgent enqueue).
    pub current_tool_call_id: Option<String>,
    /// Agent skills directory for resolving skill paths (e.g. `skills/plagiarism/`).
    pub skills_dir: Option<PathBuf>,
    /// `{agent_root}/.novel-agent/api_config.json` — WebSearch loads API key when env is unset.
    pub global_api_config_path: Option<PathBuf>,
}

impl ToolContext {
    pub fn new(project_root: PathBuf) -> Self {
        Self {
            permission_mode: PermissionMode::Normal,
            deny_rules: vec![],
            always_allow: vec![
                "CharacterSearch".into(),
                "PlotGraph".into(),
                "Tail".into(),
                "TodoWrite".into(),
            ],
            project_root,
            session_id: "local".into(),
            db: None,
            permission_mode_override: None,
            read_file_cache: None,
            file_op_locks: None,
            allow_fork: false,
            subagent_queue: None,
            current_tool_call_id: None,
            skills_dir: None,
            global_api_config_path: None,
        }
    }

    /// Same priority as `novel-core` LLM init (`novel_config::resolve_agent_api_key`).
    pub fn resolve_deepseek_api_key(&self) -> Option<String> {
        match &self.global_api_config_path {
            Some(path) => novel_config::resolve_agent_api_key(path),
            None => std::env::var("DEEPSEEK_API_KEY")
                .ok()
                .filter(|k| !k.is_empty()),
        }
    }

    /// Returns deny reason if `deny_rules` blocks this tool or path pattern.
    pub fn deny_rule_block(&self, tool_name: &str, target_path: Option<&str>) -> Option<String> {
        for rule in &self.deny_rules {
            if rule == "*" || rule == tool_name {
                return Some(format!("deny_rules: {rule}"));
            }
            if let Some(path) = target_path {
                if rule.starts_with("Write(")
                    && (tool_name == "Write" || tool_name == "Edit")
                    && path_matches_rule(rule, path)
                {
                    return Some(format!("deny_rules: {rule}"));
                }
            }
        }
        None
    }

    pub fn effective_permission_mode(&self) -> PermissionMode {
        if let Some(lock) = &self.permission_mode_override {
            if let Ok(guard) = lock.lock() {
                return guard.clone();
            }
        }
        self.permission_mode.clone()
    }

    /// Whether a tool `path` argument targets the project `plan/` directory.
    pub fn is_under_plan_dir(path: &str) -> bool {
        let norm = normalize_rel_path(path);
        norm == "plan" || norm.starts_with("plan/")
    }

    pub fn is_tool_always_allowed(&self, tool_name: &str) -> bool {
        self.always_allow.iter().any(|n| n == tool_name)
    }

    /// Fork sub-agent tool union (+ settings `always_allow`): skip mode-specific policies
    /// (Plan `plan/` write restriction, Normal read-before-write, etc.).
    pub fn tool_skips_mode_policy(&self, tool_name: &str) -> bool {
        self.is_tool_always_allowed(tool_name)
    }

    /// In Plan mode, Write/Edit may only touch `plan/` (unless `tool_skips_mode_policy`).
    pub fn validate_plan_mode_write_path(
        &self,
        tool_name: &str,
        path: &str,
    ) -> Result<(), ToolError> {
        if self.tool_skips_mode_policy(tool_name) {
            return Ok(());
        }
        if !matches!(self.effective_permission_mode(), PermissionMode::Plan) {
            return Ok(());
        }
        if Self::is_under_plan_dir(path) {
            return Ok(());
        }
        Err(ToolError::PermissionDenied(
            "plan mode: Write/Edit only allowed under plan/ — switch permission mode in the UI or use plan/<file>"
                .into(),
        ))
    }

    pub fn resolve_path(&self, rel: &str) -> PathBuf {
        resolve_under_project(&self.project_root, rel)
    }

    /// Serialize disk + cache ops for one path (Read∥Edit races).
    pub async fn with_file_lock<F, Fut, T>(&self, path: &Path, f: F) -> T
    where
        F: FnOnce() -> Fut,
        Fut: Future<Output = T>,
    {
        let Some(locks) = &self.file_op_locks else {
            return f().await;
        };
        let lock = locks
            .entry(path.to_path_buf())
            .or_insert_with(|| Arc::new(tokio::sync::Mutex::new(())))
            .clone();
        let guard = lock.lock().await;
        let out = f().await;
        drop(guard);
        out
    }

    /// Store read cache; merges partial windows when `disk_full` or `premerged_raw` supplied.
    pub fn store_read_cache(
        &self,
        path: &Path,
        entry: ReadCacheEntry,
        disk_full: Option<&str>,
        premerged_raw: Option<&str>,
    ) -> Result<(), ToolError> {
        if let Some(cache) = &self.read_file_cache {
            let existing = cache.get(path).map(|e| e.clone());
            let final_entry =
                merge_read_cache_on_store(existing.as_ref(), entry, disk_full, premerged_raw);
            cache.insert(path.to_path_buf(), final_entry);
        }
        Ok(())
    }

    /// Direct insert without merge (Write refresh, tests).
    pub fn store_read_cache_direct(&self, path: &Path, entry: ReadCacheEntry) {
        if let Some(cache) = &self.read_file_cache {
            cache.insert(path.to_path_buf(), entry);
        }
    }

    pub fn read_cache_entry(&self, path: &PathBuf) -> Option<ReadCacheEntry> {
        self.read_file_cache
            .as_ref()
            .and_then(|c| c.get(path).map(|e| e.clone()))
    }

    /// After Read/Tail tool_result is in the transcript, record that tool's line span.
    pub fn promote_read_cache_committed(
        &self,
        path: &Path,
        tool_name: &str,
        input: &serde_json::Value,
    ) {
        let Some(cache) = &self.read_file_cache else {
            return;
        };
        if let Some(mut entry) = cache.get_mut(path) {
            if entry.is_full_read() {
                entry.commit_to_transcript();
            } else if let Some(span) = span_from_tool_input(tool_name, input, entry.total_lines) {
                entry.commit_span(span);
            }
        }
    }

    /// Call when persisting a successful Read/Tail tool_result to messages.
    pub fn promote_read_cache_for_tool_result(&self, tool_name: &str, input: &serde_json::Value) {
        if tool_name != "Read" && tool_name != "Tail" {
            return;
        }
        let Some(path) = optional_file_path(input) else {
            return;
        };
        self.promote_read_cache_committed(&self.resolve_path(&path), tool_name, input);
    }

    pub fn was_read(&self, path: &PathBuf) -> bool {
        self.read_file_cache
            .as_ref()
            .is_some_and(|c| c.contains_key(path))
    }

    /// Normal mode read-before-write. Set `only_if_exists` for Write (skip new files).
    pub fn require_read_before_write(
        &self,
        tool_name: &str,
        full: &PathBuf,
        path: &str,
        action: &str,
        only_if_exists: bool,
    ) -> Result<(), ToolError> {
        if self.tool_skips_mode_policy(tool_name) {
            return Ok(());
        }
        if matches!(
            self.effective_permission_mode(),
            PermissionMode::Auto | PermissionMode::Plan | PermissionMode::Unattended
        ) {
            return Ok(());
        }
        if only_if_exists && !full.exists() {
            return Ok(());
        }
        if !self.was_read(full) {
            return Err(ToolError::Execution(format!(
                "Read {path} before {action} (read-before-write policy)"
            )));
        }
        Ok(())
    }

    /// True if cached mtime matches and the same offset/limit was read before.
    pub fn read_dedup_hit(
        &self,
        path: &PathBuf,
        offset: Option<usize>,
        limit: Option<usize>,
        current_mtime: u64,
    ) -> bool {
        let Some(entry) = self.read_cache_entry(path) else {
            return false;
        };
        let (off, lim) = read_range_key(offset, limit);
        entry.source.is_dedup_eligible()
            && entry.mtime_secs == current_mtime
            && entry.same_range(off, lim)
    }

    /// Tail dedup: same mtime, range, total line count, and dedup-eligible source.
    pub fn tail_dedup_hit(
        &self,
        path: &PathBuf,
        start_line: usize,
        take: usize,
        total_lines: usize,
        current_mtime: u64,
    ) -> bool {
        let Some(entry) = self.read_cache_entry(path) else {
            return false;
        };
        entry.source.is_dedup_eligible()
            && entry.mtime_secs == current_mtime
            && entry.offset == Some(start_line)
            && entry.limit == Some(take)
            && entry.total_lines == total_lines
    }

    /// After Edit/Write, refresh cache with new file content (full read semantics).
    pub fn refresh_cache_after_write(&self, path: &Path, raw_content: &str, mtime_secs: u64) {
        let total_lines = if raw_content.is_empty() {
            0
        } else {
            raw_content.lines().count()
        };
        let mut entry = ReadCacheEntry {
            mtime_secs,
            raw_content: raw_content.to_string(),
            offset: None,
            limit: None,
            total_lines,
            source: ReadCacheSource::WriteRefresh,
            transcript_committed: false,
            committed_spans: Vec::new(),
            committed_offset: None,
            committed_limit: None,
        };
        entry.commit_to_transcript();
        self.store_read_cache_direct(path, entry);
    }

    /// Patch session read cache after Edit (partial slice preserved).
    #[allow(clippy::too_many_arguments)]
    pub fn patch_cache_after_edit(
        &self,
        path: &Path,
        updated_disk: &str,
        mtime_secs: u64,
        old_string: &str,
        new_string: &str,
        replace_all: bool,
        occurrences_replaced: usize,
    ) {
        let Some(mut entry) = self.read_cache_entry(&path.to_path_buf()) else {
            return;
        };
        patch_read_cache_after_edit(
            &mut entry,
            updated_disk,
            mtime_secs,
            old_string,
            new_string,
            replace_all,
            occurrences_replaced,
        );
        self.store_read_cache_direct(path, entry);
    }

    // Paths within project root that must never be written or edited.
    // Protects version control, IDE agent config, and shell config files.
    const PROTECTED_PATHS: &[&str] = &[
        ".git",
        ".claude",
        ".cursor",
        ".gitconfig",
        ".gitmodules",
        ".bashrc",
        ".bash_profile",
        ".zshrc",
        ".zprofile",
        ".profile",
    ];

    /// Validate that a write/edit target is within the session's project root
    /// and not targeting a protected path. Symlink resolution prevents bypass.
    /// This check is bypass-immune (applies even in Yolo/DontAsk modes).
    pub fn validate_write_root(&self, path: &Path) -> Result<(), ToolError> {
        // Step 1: resolve symlinks to prevent bypass
        let canonical = if path.exists() {
            path.canonicalize().unwrap_or_else(|_| path.to_path_buf())
        } else if let Some(parent) = path.parent() {
            if parent.exists() {
                parent
                    .canonicalize()
                    .ok()
                    .and_then(|p| path.file_name().map(|name| p.join(name)))
                    .unwrap_or_else(|| path.to_path_buf())
            } else {
                path.to_path_buf()
            }
        } else {
            path.to_path_buf()
        };
        let root_canonical = self
            .project_root
            .canonicalize()
            .unwrap_or_else(|_| self.project_root.clone());

        // Step 2: must be within project root
        if !canonical.starts_with(&root_canonical) {
            return Err(ToolError::PermissionDenied(format!(
                "Write denied: {} is outside project root {}",
                path.display(),
                root_canonical.display()
            )));
        }

        // Step 3: check protected paths (bypass-immune)
        let Ok(relative) = canonical.strip_prefix(&root_canonical) else {
            return Ok(()); // shouldn't happen after starts_with check
        };
        for component in relative.components() {
            if let std::path::Component::Normal(c) = component {
                let name = c.to_string_lossy();
                if Self::PROTECTED_PATHS.iter().any(|p| *p == name.as_ref()) {
                    return Err(ToolError::PermissionDenied(format!(
                        "Write denied: {} is a protected path",
                        path.display()
                    )));
                }
            }
        }
        // Also check if the file name itself is protected (e.g. writing to root of project)
        if let Some(name) = relative.file_name().and_then(|n| n.to_str()) {
            if Self::PROTECTED_PATHS.contains(&name) {
                return Err(ToolError::PermissionDenied(format!(
                    "Write denied: {} is a protected path",
                    path.display()
                )));
            }
        }
        Ok(())
    }
}

fn path_matches_rule(rule: &str, path: &str) -> bool {
    let norm = normalize_rel_path(path);
    if rule.contains("chapters/**") {
        return norm.contains("chapters/");
    }
    false
}

impl std::fmt::Debug for ToolContext {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ToolContext")
            .field("permission_mode", &self.permission_mode)
            .field("project_root", &self.project_root)
            .field("session_id", &self.session_id)
            .field("read_file_cache", &self.read_file_cache.is_some())
            .finish_non_exhaustive()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rstest::rstest;

    fn ctx_with_deny_rules(rules: &[&str]) -> ToolContext {
        let mut ctx = ToolContext::new(std::path::PathBuf::from("."));
        ctx.deny_rules = rules.iter().map(|s| (*s).to_string()).collect();
        ctx
    }

    #[rstest]
    #[case(&[], "Read", None, false)]
    #[case(&["*"], "Read", None, true)]
    #[case(&["Edit"], "Edit", None, true)]
    #[case(&["Edit"], "Grep", None, false)]
    #[case(&["Write(chapters/**)"], "Write", Some("chapters/chapter-001.md"), true)]
    #[case(&["Write(chapters/**)"], "Edit", Some("chapters/chapter-001.md"), true)]
    #[case(&["Write(chapters/**)"], "Write", Some("plan/outline.md"), false)]
    #[case(&["Write(chapters/**)"], "Read", Some("chapters/chapter-001.md"), false)]
    fn deny_rule_block_matrix(
        #[case] rules: &[&str],
        #[case] tool_name: &str,
        #[case] target_path: Option<&str>,
        #[case] blocked: bool,
    ) {
        let ctx = ctx_with_deny_rules(rules);
        assert_eq!(
            ctx.deny_rule_block(tool_name, target_path).is_some(),
            blocked
        );
    }

    #[rstest]
    #[case("chapters/foo.md", true)]
    #[case("plan/foo.md", false)]
    fn path_matches_write_chapters_rule(#[case] path: &str, #[case] matches: bool) {
        assert_eq!(path_matches_rule("Write(chapters/**)", path), matches);
    }
}
