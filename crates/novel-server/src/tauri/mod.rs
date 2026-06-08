//! Tauri IPC bridge (optional `tauri` feature — compiles without npm/frontend).
//!
//! Engine access goes through a single-task command loop (`engine_loop`).
//! Do not await user approval while holding any engine lock — approval uses `ApproveTool` commands.

mod commands;
mod dto;
mod engine_loop;
mod event_payload;
mod events;
mod session_api;
mod state;
mod stream_coalesce;

pub use commands::*;
pub use dto::{SessionTranscriptLayout, UiMessage, UiTurnBundle};
pub use engine_loop::{AppStatus, WorkSummary};
pub use state::{AppState, CommandContext};
