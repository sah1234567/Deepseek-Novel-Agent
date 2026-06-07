#![deny(clippy::unwrap_used)]
#![cfg_attr(test, allow(clippy::unwrap_used))]

mod abort;
mod blocking;
mod builtin;
mod context;
mod error;
mod executor;
mod novel;
mod paths;
mod read_cache;
mod read_economy;
mod registry;
mod tool_error_hints;
mod tool_result_format;
mod tool_result_middleware;
mod trait_def;

#[cfg(test)]
mod tests_extra;

pub use abort::{abort_channel, AbortSignal, AbortWatch, InterruptBehavior, REJECT_MESSAGE};
pub use blocking::{create_dir_all, read_to_string, run_blocking, write};
pub use builtin::{AskQuestion, AskUserQuestionPayload};
pub use context::{
    PendingSubagentWork, PermissionMode, PermissionResult, SubagentWorkQueue, ToolContext,
};
pub(crate) use error::{optional_str_any, require_str, require_str_any};
pub use error::{ToolError, ValidationError};
pub use executor::{StreamingToolExecutor, ToolCallSpec, ToolExecutor};
pub use paths::{
    extract_file_path, normalize_chapter_progress_path, normalize_rel_path, optional_file_path,
    optional_search_root, resolve_under_project,
};
pub use read_cache::{
    add_line_numbers, file_mtime_secs, read_range_key, ReadCacheEntry, ReadCacheSource,
    FILE_UNCHANGED_STUB,
};
pub use registry::ToolRegistry;
pub use tool_result_format::{
    format_tool_result_for_llm, FormattedToolResult, ToolResultSpec, NEEDS_USER_INPUT_STUB,
};
pub use tool_result_middleware::format_interrupted_tool_result;
pub use trait_def::{Tool, ToolOutput};

/// Built-in + novel tool registry (project path lives on `ToolContext`, not here).
pub fn default_registry() -> ToolRegistry {
    let mut reg = ToolRegistry::new();
    reg.register(Box::new(builtin::ReadTool));
    reg.register(Box::new(builtin::TailTool));
    reg.register(Box::new(builtin::WriteTool));
    reg.register(Box::new(builtin::EditTool));
    reg.register(Box::new(builtin::GrepTool));
    reg.register(Box::new(builtin::GlobTool));
    reg.register(Box::new(builtin::BashTool));
    reg.register(Box::new(builtin::TodoWriteTool));
    reg.register(Box::new(builtin::AskUserQuestionTool));
    reg.register(Box::new(builtin::WebSearchTool));
    reg.register(Box::new(builtin::InvokeSkillTool));
    reg.register(Box::new(novel::CharacterSearchTool));
    reg.register(Box::new(novel::PlotGraphTool));
    reg.register(Box::new(novel::PlotGridTool));
    reg.register(Box::new(novel::ForeshadowTrackerTool));
    reg.register(Box::new(novel::StatsTool));
    reg.register(Box::new(novel::CorkboardTool));
    reg.register(Box::new(novel::CharacterRotateTool));
    reg.register(Box::new(novel::ForkSubAgentTool));
    reg.register(Box::new(novel::ImpactAnalysisTool));
    reg.register(Box::new(novel::KnowledgeDeriveTool));
    reg.register(Box::new(novel::TrackingQueryTool));
    reg.register(Box::new(novel::RelationQueryTool));
    reg
}
