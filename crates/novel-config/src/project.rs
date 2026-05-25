use super::{HookConfig, ModelConfig};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ProjectSettings {
    #[serde(default)]
    pub project: ProjectMeta,
    #[serde(default)]
    pub model: ModelConfig,
    #[serde(default)]
    pub hooks: HookConfig,
    #[serde(default)]
    pub permissions: PermissionsConfig,
    #[serde(default)]
    pub agent: AgentConfig,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ProjectMeta {
    pub title: String,
    pub author: String,
    #[serde(default)]
    pub genre: Vec<String>,
    #[serde(default = "default_lang")]
    pub language: String,
}

fn default_lang() -> String {
    "zh-CN".into()
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PermissionsConfig {
    pub mode: String,
    #[serde(default)]
    pub deny_rules: Vec<String>,
    #[serde(default)]
    pub always_allow: Vec<String>,
}

impl Default for PermissionsConfig {
    fn default() -> Self {
        Self {
            mode: "default".into(),
            deny_rules: vec![],
            always_allow: vec![
                "CharacterSearch".into(),
                "PlotGraph".into(),
                "ChapterRead".into(),
                "TodoWrite".into(),
            ],
        }
    }
}

fn default_max_tool_concurrency() -> usize {
    10
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentConfig {
    pub consistency_checker_max_turns: u32,
    #[serde(default = "default_max_tool_concurrency")]
    pub max_tool_concurrency: usize,
}

impl Default for AgentConfig {
    fn default() -> Self {
        Self {
            consistency_checker_max_turns: 50,
            max_tool_concurrency: default_max_tool_concurrency(),
        }
    }
}
