//! Test-only helpers (not compiled in release library consumers).

/// Temporarily remove `DEEPSEEK_API_KEY` so unit tests use the offline LLM mock.
#[cfg(test)]
pub struct StripDeepseekApiKey(Option<String>);

#[cfg(test)]
impl StripDeepseekApiKey {
    pub fn new() -> Self {
        let saved = std::env::var("DEEPSEEK_API_KEY").ok();
        std::env::remove_var("DEEPSEEK_API_KEY");
        Self(saved)
    }
}

#[cfg(test)]
impl Drop for StripDeepseekApiKey {
    fn drop(&mut self) {
        if let Some(key) = self.0.take() {
            std::env::set_var("DEEPSEEK_API_KEY", key);
        }
    }
}
