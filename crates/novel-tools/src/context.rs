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

/// Queued fork requests from main-session `ForkSubAgent` tool calls.
pub type ForkQueue = Arc<Mutex<Vec<(String, String)>>>;

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
    /// Value: (mtime_secs, cached_content) — mtime used for dedup, content for reference
    pub read_file_cache: Option<Arc<dashmap::DashMap<PathBuf, (u64, String)>>>,
    /// Main session only; false while any sub-agent inner loop runs (blocks nested fork).
    pub allow_fork: bool,
    /// Engine fork queue; present only on main-session tool context.
    pub fork_queue: Option<ForkQueue>,
    /// Agent skills directory for resolving skill paths (e.g. `skills/plagiarism/`).
    pub skills_dir: Option<PathBuf>,
}

impl ToolContext {
    pub fn new(project_root: PathBuf) -> Self {
        Self {
            permission_mode: PermissionMode::Normal,
            deny_rules: vec![],
            always_allow: vec![
                "CharacterSearch".into(),
                "PlotGraph".into(),
                "ChapterRead".into(),
                "TodoWrite".into(),
            ],
            project_root,
            session_id: "local".into(),
            db: None,
            permission_mode_override: None,
            read_file_cache: None,
            allow_fork: false,
            fork_queue: None,
            skills_dir: None,
        }
    }

    /// Returns deny reason if `deny_rules` blocks this tool or path pattern.
    pub fn deny_rule_block(&self, tool_name: &str, target_path: Option<&str>) -> Option<String> {
        for rule in &self.deny_rules {
            if rule == "*" || rule == tool_name {
                return Some(format!("deny_rules: {rule}"));
            }
            if let Some(path) = target_path {
                if rule.starts_with("Write(") && (tool_name == "Write" || tool_name == "Edit") {
                    if path_matches_rule(rule, path) {
                        return Some(format!("deny_rules: {rule}"));
                    }
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
        let norm = path.replace('\\', "/");
        let norm = norm.trim_start_matches("./");
        norm == "plan" || norm.starts_with("plan/")
    }

    /// In Plan mode, Write/Edit may only touch `plan/`.
    pub fn validate_plan_mode_write_path(&self, path: &str) -> Result<(), ToolError> {
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
        let normalized = rel.replace('/', std::path::MAIN_SEPARATOR_STR);
        let p = PathBuf::from(normalized);
        if p.is_absolute() {
            p
        } else {
            self.project_root.join(p)
        }
    }

    pub fn is_tool_always_allowed(&self, tool_name: &str) -> bool {
        self.always_allow.iter().any(|n| n == tool_name)
    }

    pub fn mark_read(&self, path: &PathBuf) {
        if let Some(cache) = &self.read_file_cache {
            cache.entry(path.clone()).or_insert((0, String::new()));
        }
    }

    pub fn was_read(&self, path: &PathBuf) -> bool {
        self.read_file_cache
            .as_ref()
            .is_some_and(|c| c.contains_key(path))
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
                    .and_then(|p| {
                        path.file_name().map(|name| p.join(name))
                    })
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
            if Self::PROTECTED_PATHS.iter().any(|p| *p == name) {
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
    let norm = path.replace('\\', "/");
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
