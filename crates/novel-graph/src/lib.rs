#![deny(clippy::unwrap_used)]
#![cfg_attr(test, allow(clippy::unwrap_used))]
#![cfg_attr(test, allow(clippy::expect_used))]

//! Generic DAG+Loop workflow execution engine.
//! Depends only on `novel-config` (+ serde stack). Must not depend on novel-core/tools/server.

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

pub use error::{GraphError, GraphResult};
pub use gate::check_write_allowed;
pub use init::ensure_graph_initialized;
pub use persist::{
    append_jsonl, list_handoff_snapshots, load_plan, load_state, plan_exists, plan_path,
    save_handoff, save_handoff_snapshot, save_plan, save_state, state_path,
};
pub use regate::parse_regate_directive;
pub use snapshot::{build_snapshot, empty_snapshot};
pub use template::render_template;
pub use tracker::{GraphTracker, LoopAdvanceEvent};
pub use types::*;
pub use validate::{parse_plan, validate_plan};

/// Default scaffold plan JSON (empty skeleton — LLM builds workflow via PlanBuilder).
pub fn default_plan_json() -> &'static str {
    include_str!("../templates/plan-graph.json")
}

/// Default empty skeleton — no nodes, no loops. LLM builds workflow via PlanBuilder.
/// Does NOT validate (empty plan is a valid initial state).
pub fn default_plan() -> GraphResult<PlanGraph> {
    serde_json::from_str(default_plan_json()).map_err(|e| GraphError::Validation(e.to_string()))
}
