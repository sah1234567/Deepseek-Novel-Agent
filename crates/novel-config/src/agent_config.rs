use crate::ConfigError;
use serde::{Deserialize, Serialize};
use std::path::Path;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct AgentApiConfig {
    pub api_key: String,
    #[serde(default = "default_api_base")]
    pub api_base: String,
}

fn default_api_base() -> String {
    // Keep in sync with `novel-deepseek/config.toml` ? deepseek.chat_api_base
    "https://api.deepseek.com/v1".into()
}

impl Default for AgentApiConfig {
    fn default() -> Self {
        Self {
            api_key: String::new(),
            api_base: default_api_base(),
        }
    }
}

pub fn load_agent_api_config(
    path: impl AsRef<Path>,
) -> Result<Option<AgentApiConfig>, ConfigError> {
    let path = path.as_ref();
    if !path.exists() {
        return Ok(None);
    }
    let content = std::fs::read_to_string(path).map_err(ConfigError::Io)?;
    let cfg: AgentApiConfig = serde_json::from_str(&content).map_err(ConfigError::Json)?;
    Ok(Some(cfg))
}

pub fn save_agent_api_config(
    path: impl AsRef<Path>,
    cfg: &AgentApiConfig,
) -> Result<(), ConfigError> {
    let path = path.as_ref();
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(ConfigError::Io)?;
    }
    let content = serde_json::to_string_pretty(cfg).map_err(ConfigError::Json)?;
    std::fs::write(path, content).map_err(ConfigError::Io)
}

/// `DEEPSEEK_API_KEY` env, then non-empty key in `api_config.json`.
pub fn resolve_agent_api_key(config_path: impl AsRef<Path>) -> Option<String> {
    if let Ok(key) = std::env::var("DEEPSEEK_API_KEY") {
        if !key.is_empty() {
            return Some(key);
        }
    }
    load_agent_api_config(config_path)
        .ok()
        .flatten()
        .filter(|c| !c.api_key.is_empty())
        .map(|c| c.api_key)
}

/// `DEEPSEEK_API_BASE` env, then `api_config.json`, else `default_api_base()`.
pub fn resolve_agent_api_base(config_path: impl AsRef<Path>) -> String {
    if let Ok(base) = std::env::var("DEEPSEEK_API_BASE") {
        if !base.is_empty() {
            return base;
        }
    }
    load_agent_api_config(config_path)
        .ok()
        .flatten()
        .map(|c| c.api_base)
        .filter(|b| !b.is_empty())
        .unwrap_or_else(default_api_base)
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn resolve_api_key_env_over_file() {
        let tmp = TempDir::new().unwrap();
        let path = tmp.path().join("api_config.json");
        save_agent_api_config(
            &path,
            &AgentApiConfig {
                api_key: "from-file".into(),
                ..Default::default()
            },
        )
        .unwrap();
        std::env::set_var("DEEPSEEK_API_KEY", "from-env");
        assert_eq!(resolve_agent_api_key(&path).as_deref(), Some("from-env"));
        std::env::remove_var("DEEPSEEK_API_KEY");
    }

    #[test]
    fn resolve_api_key_from_file() {
        let tmp = TempDir::new().unwrap();
        let path = tmp.path().join("api_config.json");
        save_agent_api_config(
            &path,
            &AgentApiConfig {
                api_key: "file-key".into(),
                ..Default::default()
            },
        )
        .unwrap();
        std::env::remove_var("DEEPSEEK_API_KEY");
        assert_eq!(resolve_agent_api_key(&path).as_deref(), Some("file-key"));
    }

    #[test]
    fn resolve_api_key_missing_file_returns_none() {
        std::env::remove_var("DEEPSEEK_API_KEY");
        let path = std::env::temp_dir().join("novel-agent-missing-api-config.json");
        assert!(resolve_agent_api_key(&path).is_none());
    }

    #[test]
    fn roundtrip_api_config() {
        let tmp = TempDir::new().unwrap();
        let path = tmp.path().join("api_config.json");
        let cfg = AgentApiConfig {
            api_key: "sk-test".into(),
            api_base: "https://api.deepseek.com/v1".into(),
        };
        save_agent_api_config(&path, &cfg).unwrap();
        let loaded = load_agent_api_config(&path).unwrap().unwrap();
        assert_eq!(loaded, cfg);
    }
}
