//! User interrupt, stream cancel finalization, and LLM abort signal mapping.

mod abort_map;
mod controller;
pub(crate) mod finalize;

pub(crate) use abort_map::map_abort_signal;
pub use controller::{AbortController, InterruptReason, ERROR_MESSAGE_USER_ABORT};
