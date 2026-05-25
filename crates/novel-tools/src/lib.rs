#![warn(clippy::unwrap_used)]
#![cfg_attr(test, allow(clippy::unwrap_used))]

mod abort;
mod blocking;
mod builtin;
mod context;
mod error;
mod executor;
mod novel;
mod registry;
mod trait_def;

#[cfg(test)]
mod tests_extra;

pub use abort::{abort_channel, AbortSignal, AbortWatch, InterruptBehavior, REJECT_MESSAGE};
pub use blocking::{create_dir_all, read_to_string, run_blocking, write};
pub use builtin::AskUserQuestionPayload;
pub use context::{ForkQueue, PermissionMode, PermissionResult, ToolContext};
pub use error::{optional_str_any, require_str, require_str_any, ToolError, ValidationError};
pub use executor::{
    StreamingToolExecutor, ToolCallSpec, ToolExecutor, DEFAULT_MAX_CONCURRENT_TOOLS,
};
pub use registry::ToolRegistry;
pub use trait_def::{Tool, ToolOutput};

pub fn default_registry(_project_root: std::path::PathBuf) -> ToolRegistry {
    let mut reg = ToolRegistry::new();
    reg.register(Box::new(builtin::ReadTool));
    reg.register(Box::new(builtin::WriteTool));
    reg.register(Box::new(builtin::EditTool));
    reg.register(Box::new(builtin::GrepTool));
    reg.register(Box::new(builtin::GlobTool));
    reg.register(Box::new(builtin::BashTool));
    reg.register(Box::new(builtin::TodoWriteTool));
    reg.register(Box::new(builtin::AskUserQuestionTool));
    reg.register(Box::new(novel::CharacterSearchTool));
    reg.register(Box::new(novel::PlotGraphTool));
    reg.register(Box::new(novel::ConsistencyCheckTool));
    reg.register(Box::new(novel::ChapterReadTool));
    reg.register(Box::new(novel::WebSearchTool));
    reg.register(Box::new(novel::PlotGridTool));
    reg.register(Box::new(novel::ForeshadowTrackerTool));
    reg.register(Box::new(novel::StatsTool));
    reg.register(Box::new(novel::CorkboardTool));
    reg.register(Box::new(novel::CharacterRotateTool));
    reg.register(Box::new(novel::ForkSubAgentTool));
    reg.register(Box::new(novel::InvokeSkillTool));
    reg.register(Box::new(novel::ImpactAnalysisTool));
    reg.register(Box::new(novel::KnowledgeDeriveTool));
    reg
}
