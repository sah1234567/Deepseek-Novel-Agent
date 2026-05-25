use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct HookConfig {
    #[serde(default)]
    pub post_tool_use: Vec<HookMatcher>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HookMatcher {
    pub matcher: String,
    #[serde(default)]
    pub hooks: Vec<HookRule>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HookRule {
    #[serde(rename = "type")]
    pub hook_type: String,
    pub prompt: String,
    #[serde(default = "default_timeout")]
    pub timeout: u32,
}

fn default_timeout() -> u32 {
    60
}
