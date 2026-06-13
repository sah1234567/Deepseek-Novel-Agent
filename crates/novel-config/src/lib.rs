#![deny(clippy::unwrap_used)]
#![cfg_attr(test, allow(clippy::unwrap_used))]

mod agent_config;
mod error;
mod fork_agents;
mod hook;
mod model;
mod paths;
mod project;

pub use agent_config::{
    load_agent_api_config, resolve_agent_api_base, resolve_agent_api_key, save_agent_api_config,
    AgentApiConfig,
};
pub use error::ConfigError;
pub use fork_agents::{
    is_forkable_agent_type, CHAPTER_CRAFT_ANALYZER_MAX_REACT_LOOPS, FORKABLE_AGENT_TYPE_NAMES,
    GENERAL_PURPOSE_MAX_REACT_LOOPS, KNOWLEDGE_AUDITOR_MAX_REACT_LOOPS_DEFAULT,
    PLAN_AUDITOR_MAX_REACT_LOOPS,
};
pub use hook::{HookConfig, HookMatcher, HookRule};
pub use model::{ModelConfig, Provider};
pub use paths::{
    ensure_work_under_works, global_api_config_path, global_config_dir, resolve_agent_root,
    skills_dir, templates_dir, validate_work_name, work_path, works_dir,
};
pub use project::{AgentConfig, PermissionsConfig, ProjectMeta, ProjectSettings};

use figment::{providers::Env, Figment};
use std::path::Path;

/// Load project settings from `settings.json` with `NOVEL_*` env overrides.
pub fn load_project_settings(path: impl AsRef<Path>) -> Result<ProjectSettings, ConfigError> {
    let path = path.as_ref();
    if !path.exists() {
        return Ok(ProjectSettings::default());
    }
    let content = std::fs::read_to_string(path).map_err(ConfigError::Io)?;
    let mut settings: ProjectSettings =
        serde_json::from_str(&content).map_err(ConfigError::Json)?;
    settings.apply_env_overrides();
    settings.validate()?;
    Ok(settings)
}

impl ProjectSettings {
    pub(crate) fn apply_env_overrides(&mut self) {
        let env = Figment::new().merge(Env::prefixed("NOVEL_"));
        if let Ok(api_base) = env.extract_inner::<String>("API_BASE") {
            self.model.api_base = api_base;
        }
        if let Ok(model) = env.extract_inner::<String>("MODEL") {
            self.model.model = model;
        }
        if let Ok(threshold) = env.extract_inner::<f32>("COMPACTION_THRESHOLD") {
            self.model.compaction_threshold = threshold;
        }
        if let Ok(v) = std::env::var("NOVEL_THINKING_ENABLED") {
            if let Ok(b) = v.parse::<bool>() {
                self.model.thinking_enabled = b;
            }
        }
        if let Ok(v) = std::env::var("NOVEL_MAX_OUTPUT_TOKENS") {
            if let Ok(n) = v.parse::<u32>() {
                self.model.max_output_tokens = n;
            }
        }
        if let Ok(v) = std::env::var("NOVEL_CONTEXT_WINDOW_SIZE") {
            if let Ok(n) = v.parse::<usize>() {
                self.model.context_window_size = n;
            }
        }
    }

    fn validate(&self) -> Result<(), ConfigError> {
        if self.model.context_window_size == 0 {
            return Err(ConfigError::InvalidValue(
                "context_window_size must be > 0".into(),
            ));
        }
        if !(0.0..=1.0).contains(&self.model.compaction_threshold) {
            return Err(ConfigError::InvalidValue(
                "compaction_threshold must be in [0, 1]".into(),
            ));
        }
        if self.agent.max_tool_concurrency == 0 || self.agent.max_tool_concurrency > 32 {
            return Err(ConfigError::InvalidValue(
                "max_tool_concurrency must be in 1..=32".into(),
            ));
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rstest::rstest;
    use std::io::Write;
    use tempfile::NamedTempFile;

    #[rstest]
    #[test]
    fn default_settings_valid() {
        let s = ProjectSettings::default();
        s.validate().unwrap();
        assert_eq!(s.model.provider, Provider::Deepseek);
        assert_eq!(s.model.context_window_size, 1_000_000);
    }

    #[rstest]
    #[test]
    fn load_missing_file_returns_default() {
        let settings = load_project_settings("/nonexistent/settings.json").unwrap();
        assert_eq!(settings.model.model, "deepseek-v4-pro");
    }

    #[rstest]
    #[test]
    fn load_invalid_json_errors() {
        let mut f = NamedTempFile::new().unwrap();
        write!(f, "{{ invalid").unwrap();
        let err = load_project_settings(f.path()).unwrap_err();
        assert!(matches!(err, ConfigError::Json(_)));
    }

    #[rstest]
    #[test]
    fn invalid_compaction_threshold_rejected() {
        let mut s = ProjectSettings::default();
        s.model.compaction_threshold = 1.5;
        assert!(s.validate().is_err());
    }

    #[rstest]
    #[test]
    fn invalid_max_tool_concurrency_rejected() {
        let mut s = ProjectSettings::default();
        s.agent.max_tool_concurrency = 0;
        assert!(s.validate().is_err());
        s.agent.max_tool_concurrency = 33;
        assert!(s.validate().is_err());
    }

    #[test]
    fn env_overrides_max_output_tokens() {
        let _guard = EnvGuard::set("NOVEL_MAX_OUTPUT_TOKENS", "8192");
        let mut settings = ProjectSettings::default();
        settings.apply_env_overrides();
        assert_eq!(settings.model.max_output_tokens, 8192);
    }

    struct EnvGuard {
        key: &'static str,
        prev: Option<String>,
    }

    impl EnvGuard {
        fn set(key: &'static str, value: &str) -> Self {
            let prev = std::env::var(key).ok();
            // SAFETY: test-only; restored on drop.
            unsafe { std::env::set_var(key, value) };
            Self { key, prev }
        }
    }

    impl Drop for EnvGuard {
        fn drop(&mut self) {
            match &self.prev {
                Some(v) => unsafe { std::env::set_var(self.key, v) },
                None => unsafe { std::env::remove_var(self.key) },
            }
        }
    }
}
