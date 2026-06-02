#[derive(Debug, Clone)]
pub struct RetainPolicy {
    pub recent_react_turns: usize,
    pub summary_max_chars: usize,
    pub summary_max_output_tokens: u32,
}

impl Default for RetainPolicy {
    fn default() -> Self {
        Self {
            recent_react_turns: 5,
            summary_max_chars: 10_000,
            summary_max_output_tokens: 16_384,
        }
    }
}
