use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum Provider {
    #[default]
    Deepseek,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ModelConfig {
    pub provider: Provider,
    pub model: String,
    pub api_base: String,
    pub context_window_size: usize,
    pub compaction_threshold: f32,
    pub max_output_tokens: u32,
    /// Enable thinking/reasoning mode (DeepSeek V4-Pro / reasoner models).
    #[serde(default = "default_thinking")]
    pub thinking_enabled: bool,
}

fn default_thinking() -> bool {
    true
}

impl Default for ModelConfig {
    fn default() -> Self {
        Self {
            provider: Provider::Deepseek,
            model: "deepseek-v4-pro".into(),
            // Keep in sync with `novel-deepseek/config.toml` → deepseek.chat_api_base
            api_base: "https://api.deepseek.com/v1".into(),
            context_window_size: 1_000_000,
            compaction_threshold: 0.8,
            max_output_tokens: 64_000,
            thinking_enabled: true,
        }
    }
}
