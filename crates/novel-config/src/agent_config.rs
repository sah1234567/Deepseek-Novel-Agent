use crate::{global_api_config_path, ConfigError};
use serde::{Deserialize, Serialize};
use std::path::Path;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct AgentApiConfig {
    pub api_key: String,
    #[serde(default = "default_api_base")]
    pub api_base: String,
}

fn default_api_base() -> String {
    // Keep in sync with `novel-deepseek/config.toml` → deepseek.chat_api_base
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

pub fn load_agent_api_config(path: impl AsRef<Path>) -> Result<Option<AgentApiConfig>, ConfigError> {
    let path = path.as_ref();
    if !path.exists() {
        return Ok(None);
    }
    let content = std::fs::read_to_string(path).map_err(ConfigError::Io)?;
    let cfg: AgentApiConfig = serde_json::from_str(&content).map_err(ConfigError::Json)?;
    Ok(Some(cfg))
}

pub fn save_agent_api_config(path: impl AsRef<Path>, cfg: &AgentApiConfig) -> Result<(), ConfigError> {
    let path = path.as_ref();
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(ConfigError::Io)?;
    }
    let content = serde_json::to_string_pretty(cfg).map_err(ConfigError::Json)?;
    std::fs::write(path, content).map_err(ConfigError::Io)
}

pub fn load_agent_api_config_from_root(agent_root: impl AsRef<Path>) -> Result<Option<AgentApiConfig>, ConfigError> {
    load_agent_api_config(global_api_config_path(agent_root))
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

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
