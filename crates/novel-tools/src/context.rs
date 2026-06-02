use crate::paths::{normalize_rel_path, resolve_under_project};
use crate::read_cache::{read_range_key, ReadCacheEntry};
use crate::ToolError;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

#[derive(Debug, Clone)]
pub enum PermissionMode {
    Normal,
    Plan,
    Auto,
    Unattended,
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

    pub fn store_read_cache(&self, path: &Path, entry: ReadCacheEntry) {
        if let Some(cache) = &self.read_file_cache {
            cache.insert(path.to_path_buf(), entry);
        }
    }

    pub fn read_cache_entry(&self, path: &PathBuf) -> Option<ReadCacheEntry> {
        self.read_file_cache
            .as_ref()
            .and_then(|c| c.get(path).map(|e| e.clone()))
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

    /// Edit only: old_string must lie in the cached Read/Tail slice (full read always OK).
    pub fn require_edit_in_read_slice(
        &self,
        full: &PathBuf,
        old_string: &str,
    ) -> Result<(), ToolError> {
        let Some(entry) = self.read_cache_entry(full) else {
            return Ok(());
        };
        if entry.is_full_read() || entry.covers_edit_target(old_string) {
            return Ok(());
        }
        Err(ToolError::Execution(
            "Edit target not in the read slice (only read a portion of this file). \
             Read with offset/limit covering old_string, or Read full file."
                .into(),
        ))
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
        self.store_read_cache(
            path,
            ReadCacheEntry {
                mtime_secs,
                raw_content: raw_content.to_string(),
                offset: None,
                limit: None,
                total_lines,
                source: crate::read_cache::ReadCacheSource::WriteRefresh,
            },
        );
    }

    // Paths within project root that must never be written or edited.
    // Protects version control, Claude Code config, and shell config files.
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
