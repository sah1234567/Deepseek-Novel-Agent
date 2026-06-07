//! Main-session turn execution: inner ReAct loop, LLM streaming, tool dispatch.

mod context;
mod llm_stream;
pub mod r#loop;
mod tool_apply;
mod tool_dispatch;
mod tool_merge;

pub use context::{TurnContext, MSG_SEQ_USER};
pub(crate) use tool_dispatch::{format_tool, StreamingToolDispatch};
