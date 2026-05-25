use serde::Deserialize;
use std::sync::OnceLock;

#[derive(Debug, Clone, Deserialize)]
struct DeepSeekConfigFile {
    deepseek: DeepSeekEndpoints,
}

/// Endpoint defaults loaded from `novel-deepseek/config.toml`.
#[derive(Debug, Clone, Deserialize, PartialEq, Eq)]
pub struct DeepSeekEndpoints {
    pub chat_api_base: String,
    pub web_search_messages_url: String,
}

static EMBEDDED: OnceLock<DeepSeekEndpoints> = OnceLock::new();

fn embedded() -> &'static DeepSeekEndpoints {
    EMBEDDED.get_or_init(|| {
        let raw = include_str!(concat!(env!("CARGO_MANIFEST_DIR"), "/config.toml"));
        toml::from_str::<DeepSeekConfigFile>(raw)
            .expect("novel-deepseek/config.toml must parse")
            .deepseek
    })
}

/// Defaults from `config.toml` (compile-time embedded).
pub fn defaults() -> DeepSeekEndpoints {
    embedded().clone()
}

/// OpenAI-compatible chat base URL (`{base}/chat/completions`).
/// Env override: `DEEPSEEK_API_BASE`.
pub fn chat_api_base() -> String {
    std::env::var("DEEPSEEK_API_BASE").unwrap_or_else(|_| embedded().chat_api_base.clone())
}

/// Anthropic Messages URL for server-side web search.
/// Env override: `DEEPSEEK_WEB_SEARCH_MESSAGES_URL`.
pub fn web_search_messages_url() -> String {
    std::env::var("DEEPSEEK_WEB_SEARCH_MESSAGES_URL")
        .unwrap_or_else(|_| embedded().web_search_messages_url.clone())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn embedded_config_matches_expected_urls() {
        let d = defaults();
        assert_eq!(d.chat_api_base, "https://api.deepseek.com/v1");
        assert_eq!(
            d.web_search_messages_url,
            "https://api.deepseek.com/anthropic/v1/messages"
        );
    }
}
