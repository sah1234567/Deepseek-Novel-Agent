//! Generic DAG+Loop workflow execution engine types.
//!
//! Zero domain knowledge — no assumptions about chapters, volumes, outlines, or novels.

use serde::{Deserialize, Serialize};
use std::collections::HashMap;

pub const PLAN_GRAPH_REL: &str = "knowledge/meta/plan-graph.json";
pub const GRAPH_STATE_REL: &str = "knowledge/meta/graph-state.json";
pub const GRAPH_JSONL_REL: &str = "knowledge/meta/graph.jsonl";
pub const HANDOFFS_DIR_REL: &str = "knowledge/meta/handoffs";

// ── Cursor (generic key-value counters + tags) ──────────────────────

/// Generic execution cursor. `counters` and `tags` keys are defined by the plan JSON;
/// Rust code makes zero assumptions about their names or semantics.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct Cursor {
    pub counters: HashMap<String, i64>,
    #[serde(default)]
    pub tags: HashMap<String, String>,
}

// ── Advance rule (configurable loop progression) ────────────────────

fn one_i64() -> i64 {
    1
}

/// Describes how a loop cursor advances when `advance_after` station achieves.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct AdvanceRule {
    /// Which counter to increment on advance.
    pub increment: String,
    /// How much to increment (default 1).
    #[serde(default = "one_i64")]
    pub step: i64,
    /// Side-effects applied to other counters after the primary increment.
    #[serde(default)]
    pub side_effects: Vec<CounterOp>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "op", rename_all = "snake_case")]
pub enum CounterOp {
    Increment {
        counter: String,
        #[serde(default = "one_i64")]
        by: i64,
    },
    Decrement {
        counter: String,
        #[serde(default = "one_i64")]
        by: i64,
    },
    Set {
        counter: String,
        value: i64,
    },
    Reset {
        counter: String,
    },
}

// ── Loop termination condition ──────────────────────────────────────

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Until {
    /// counter > value (or resolved value_from)
    CounterGt {
        counter: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        value: Option<i64>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        value_from: Option<String>,
    },
    /// counter >= value
    CounterGe {
        counter: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        value: Option<i64>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        value_from: Option<String>,
    },
    /// counter < value
    CounterLt {
        counter: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        value: Option<i64>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        value_from: Option<String>,
    },
    /// counter == value
    CounterEq {
        counter: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        value: Option<i64>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        value_from: Option<String>,
    },
    /// Never auto-complete; only stopped via manual command.
    Manual,
}

// ── Artifact role ───────────────────────────────────────────────────

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum ArtifactRole {
    #[default]
    PrimaryDeliverable,
    Input,
    Aux,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ArtifactRef {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub path: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub path_template: Option<String>,
    #[serde(default)]
    pub role: ArtifactRole,
}

// ── Acceptance / gate ───────────────────────────────────────────────

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum MachineAcceptance {
    #[default]
    None,
    Verifier,
    Auditor,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum OnReject {
    Continue,
    Fail,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct HumanGate {
    pub required: bool,
    #[serde(default = "default_on_reject")]
    pub on_reject: OnReject,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub prompt: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub review: Vec<String>,
}

fn default_on_reject() -> OnReject {
    OnReject::Continue
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct Acceptance {
    #[serde(default)]
    pub machine: MachineAcceptance,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub human: Option<HumanGate>,
}

// ── Rollback / iterate ─────────────────────────────────────────────

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum RollbackTargets {
    #[default]
    None,
    #[serde(rename = "self")]
    Self_,
    BlockingAncestors,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Rollback {
    #[serde(default)]
    pub allowed_targets: RollbackTargets,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub explicit_ids: Vec<String>,
    #[serde(default)]
    pub on_reject_suggest: bool,
    #[serde(default = "default_max_regates")]
    pub max_regates: u32,
}

impl Default for Rollback {
    fn default() -> Self {
        Self {
            allowed_targets: RollbackTargets::default(),
            explicit_ids: Vec::new(),
            on_reject_suggest: false,
            max_regates: default_max_regates(),
        }
    }
}

fn default_max_regates() -> u32 {
    3
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum IterateUntil {
    #[default]
    HumanApprove,
    AuditorPass,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Iterate {
    pub max_iterations: u32,
    #[serde(default)]
    pub until: IterateUntil,
}

// ── Plan node ──────────────────────────────────────────────────────

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct PlanNode {
    pub id: String,
    pub title: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub spec: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub spec_template: Option<String>,
    #[serde(default)]
    pub deps: Vec<String>,
    /// Domain tags (e.g. "chapter_body", "world_bible", "review").
    /// Replaces the old `NodeKind` enum — any string is valid.
    #[serde(default)]
    pub tags: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub artifacts: Vec<ArtifactRef>,
    #[serde(default)]
    pub acceptance: Acceptance,
    #[serde(default)]
    pub rollback: Rollback,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub iterate: Option<Iterate>,
}

// ── Loop definition ────────────────────────────────────────────────

fn default_true() -> bool {
    true
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LoopOnAdvance {
    pub reopen: Vec<String>,
    /// Schema-retained for plan-graph compatibility; runtime now always clears per-iteration state.
    #[serde(default = "default_true")]
    pub clear_node_sessions: bool,
    /// Clear cached `effective_spec` so the next start re-renders NodeObjective (default true).
    #[serde(default = "default_true")]
    pub reinject_objectives: bool,
    /// When true (default), loop advance must not delete `world_state_board` files.
    #[serde(default = "default_true")]
    pub preserve_canon_files: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WorkflowLoop {
    pub id: String,
    pub stations: Vec<String>,
    pub entry: String,
    pub advance_after: String,
    #[serde(default)]
    pub cursor: Cursor,
    /// How the cursor advances when `advance_after` achieves.
    #[serde(default = "default_advance")]
    pub advance: AdvanceRule,
    /// Termination condition.
    pub until: Until,
    pub on_advance: LoopOnAdvance,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub world_state_board: Vec<String>,
}

/// Default advance rule: increment first counter by 1, no side effects.
/// Plan authors should override with domain-specific counters.
fn default_advance() -> AdvanceRule {
    AdvanceRule {
        increment: String::new(),
        step: 1,
        side_effects: vec![],
    }
}

// ── Plan graph ─────────────────────────────────────────────────────

fn default_max_parallel() -> usize {
    4
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct PlanGraph {
    pub version: String,
    #[serde(default)]
    pub nodes: Vec<PlanNode>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub loops: Vec<WorkflowLoop>,
    #[serde(default)]
    pub enforce_gates: bool,
    #[serde(default = "default_max_parallel")]
    pub max_parallel_nodes: usize,
    /// Generic key-value settings (e.g. targetChapters, targetSections, …).
    /// Replaces the old `target_chapters: Option<u32>`.
    #[serde(default)]
    pub settings: HashMap<String, serde_json::Value>,
    #[serde(default)]
    pub auto_start_ready: bool,
}

// ── Status / phase enums ───────────────────────────────────────────

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum NodeStatus {
    #[default]
    Waiting,
    Ready,
    Running,
    Verifying,
    AwaitingApproval,
    Achieved,
    Failed,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum LoopPhase {
    #[default]
    Idle,
    Running,
    Advancing,
    Paused,
    Completed,
}

// ── Runtime state types ────────────────────────────────────────────

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FileTouch {
    pub path: String,
    #[serde(default = "default_op_update")]
    pub op: String,
}

fn default_op_update() -> String {
    "update".into()
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct NodeHandoff {
    pub node_id: String,
    pub summary: String,
    pub files_touched: Vec<FileTouch>,
    pub artifacts: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub achieved_at: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct GraphNodeRuntime {
    pub status: NodeStatus,
    #[serde(default)]
    pub regate_count: u32,
    #[serde(default)]
    pub iteration: u32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub feedback: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub loop_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub effective_spec: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub files_touched_journal: Vec<FileTouch>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pending_summary: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub handoff: Option<NodeHandoff>,
    /// Author intervened this iteration (e.g. chat turn wrote files). Forces human gate on submit.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub human_intervened: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct GraphLoopRuntime {
    #[serde(default)]
    pub cursor: Cursor,
    #[serde(default)]
    pub phase: LoopPhase,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub active_station_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub paused_reason: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_advance_at: Option<String>,
    /// Previous iteration's advance_after handoff, preserved across loop advance
    /// so the entry node receives continuity context on the next iteration.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub prev_iteration_handoff: Option<NodeHandoff>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct GraphSettings {
    #[serde(default)]
    pub enforce_gates: bool,
    #[serde(default = "default_max_parallel")]
    pub max_parallel_nodes: usize,
    #[serde(default)]
    pub auto_start_ready: bool,
    /// Generic key-value settings.
    #[serde(default)]
    pub settings: HashMap<String, serde_json::Value>,
}

impl Default for GraphSettings {
    fn default() -> Self {
        Self {
            enforce_gates: false,
            max_parallel_nodes: default_max_parallel(),
            auto_start_ready: false,
            settings: HashMap::new(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct GraphState {
    #[serde(default)]
    pub plan_version: String,
    #[serde(default)]
    pub nodes: HashMap<String, GraphNodeRuntime>,
    #[serde(default)]
    pub loops: HashMap<String, GraphLoopRuntime>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub focused_node_id: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub running_node_ids: Vec<String>,
    #[serde(default)]
    pub settings: GraphSettings,
}

// ── IPC / UI snapshot DTOs ─────────────────────────────────────────

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GraphHitlHint {
    pub node_id: String,
    pub kind: String,
    pub label: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GraphEdgeView {
    pub source: String,
    pub target: String,
    pub kind: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GraphNodeView {
    pub id: String,
    pub title: String,
    /// Domain tags (replaces old `kind: NodeKind`).
    pub tags: Vec<String>,
    pub status: NodeStatus,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub loop_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cursor_badge: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub effective_spec: Option<String>,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub human_intervened: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GraphLoopView {
    pub loop_id: String,
    pub station_ids: Vec<String>,
    pub cursor: Cursor,
    /// Generic settings snapshot for this loop.
    #[serde(default)]
    pub settings: HashMap<String, serde_json::Value>,
    pub phase: LoopPhase,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub active_station_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub last_advance_at: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub paused_reason: Option<String>,
    pub world_state_board: Vec<String>,
    pub entry: String,
    pub advance_after: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LoopSummary {
    pub loop_id: String,
    pub cursor_label: String,
    pub phase: LoopPhase,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GraphStateSnapshot {
    pub plan_version: String,
    pub nodes: Vec<GraphNodeView>,
    pub edges: Vec<GraphEdgeView>,
    pub loops: Vec<GraphLoopView>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub focused_node_id: Option<String>,
    pub running_node_ids: Vec<String>,
    pub hitl: Vec<GraphHitlHint>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub loop_summaries: Vec<LoopSummary>,
    pub enforce_gates: bool,
    /// False when work has no formal plan-graph yet.
    #[serde(default = "default_true")]
    pub has_plan: bool,
}
