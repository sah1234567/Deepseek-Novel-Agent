#[derive(Debug, thiserror::Error)]
pub enum AgentError {
    #[error("Fork error: {0}")]
    Fork(#[from] crate::subagent::ForkError),
    #[error("Validation error: {0}")]
    Validation(String),
    #[error("Nested fork prohibited")]
    NestedForkProhibited,
    #[error("Agent busy")]
    AgentBusy,
    #[error("State error: {0}")]
    State(#[from] novel_state::StateError),
    #[error("Tool error: {0}")]
    Tool(#[from] novel_tools::ToolError),
    #[error("LLM error: {0}")]
    Llm(#[from] novel_deepseek::LlmError),
    #[error("Compaction error: {0}")]
    Compaction(#[from] novel_compaction::CompactionError),
    #[error("Config error: {0}")]
    Config(#[from] novel_config::ConfigError),
}
