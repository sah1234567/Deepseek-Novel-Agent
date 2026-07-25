#![deny(clippy::unwrap_used)]
#![cfg_attr(test, allow(clippy::unwrap_used))]

//! Graph-primary orchestration: plan schema, tracker, handoff, book loop, gates.
//! Depends only on `novel-config` (+ serde stack). Must not depend on novel-core/tools/server.

mod checkpoint;
mod error;
mod gate;
mod init;
mod persist;
mod regate;
mod snapshot;
mod template;
mod tracker;
mod types;
mod validate;

#[cfg(test)]
mod coverage_tests;

pub use checkpoint::{
    checkpoint_path, load_checkpoint, save_checkpoint, GraphCheckpoint, CHECKPOINTS_DIR_REL,
};
pub use error::{GraphError, GraphResult};
pub use gate::check_write_allowed;
pub use init::{ensure_graph_initialized, write_default_plan_file};
pub use persist::{
    append_jsonl, list_handoff_chapters, load_handoff, load_plan, load_state, plan_exists,
    plan_path, save_handoff, save_handoff_chapter, save_plan, save_state, state_path,
};
pub use regate::parse_regate_directive;
pub use snapshot::{build_snapshot, empty_snapshot};
pub use template::render_template;
pub use tracker::{GraphTracker, LoopAdvanceEvent};
pub use types::*;
pub use validate::{parse_plan, validate_plan};

/// Default scaffold plan JSON (book loop skeleton).
pub fn default_plan_json() -> &'static str {
    include_str!("../templates/plan-graph.json")
}

pub fn default_plan() -> GraphResult<PlanGraph> {
    parse_plan(default_plan_json())
}
