//! System prompt assembly, dynamic context, and agent prompt loading.

pub mod dynamic_context;

mod manager;
mod prompt_loader;
mod system_prompt;

pub use manager::ContextManager;
pub use prompt_loader::format_fork_task;
pub use system_prompt::{DynamicContext, SystemPromptBuilder};
