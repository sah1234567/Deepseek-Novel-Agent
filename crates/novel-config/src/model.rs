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

impl ModelConfig {
    /// Memory relevance selector configuration.
    ///
    /// Uses V4 Flash (not V4 Pro) because:
    /// - Classification task, not generation — no reasoning needed
    /// - 256 token output limit — cost is negligible
    /// - No thinking — latency <1s, hidden within main model's streaming delay
    /// - 128K context window is far more than the ~2K tokens actually used
    pub fn memory_selector() -> Self {
        Self {
            provider: Provider::Deepseek,
            model: "deepseek-v4-flash".into(),
            api_base: "https://api.deepseek.com/v1".into(),
            context_window_size: 128_000,
            compaction_threshold: 0.0, // never compact
            max_output_tokens: 256,    // 256 tokens ≈ 5 filenames × 50 chars
            thinking_enabled: false,   // classification doesn't need reasoning
        }
    }
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
