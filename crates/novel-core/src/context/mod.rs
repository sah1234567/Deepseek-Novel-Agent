//! System prompt assembly and session dynamic context.

pub(crate) mod dynamic_context;

mod manager;
mod system_prompt;

pub use manager::ContextManager;
pub use system_prompt::{DynamicContext, SystemPromptBuilder};
