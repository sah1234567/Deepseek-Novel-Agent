//! Main-session turn execution: inner ReAct loop, LLM streaming, tool dispatch.

/// Consecutive identical tool failures before ending the turn early.
pub(crate) const TOOL_FAILURE_CIRCUIT_THRESHOLD: u32 = 5;

mod context;
mod llm_stream;
pub(crate) mod r#loop;
mod tool_apply;
mod tool_dispatch;
mod tool_merge;

pub use context::{TurnContext, MSG_SEQ_USER, SUB_AGENT_REPORT_PREFIX};
pub(crate) use tool_dispatch::{format_tool, StreamingToolDispatch};
