//! Plan and runtime types for graph-primary orchestration.

use serde::{Deserialize, Serialize};
use std::collections::HashMap;

pub const PLAN_GRAPH_REL: &str = "knowledge/meta/plan-graph.json";
pub const GRAPH_STATE_REL: &str = "knowledge/meta/graph-state.json";
pub const GRAPH_JSONL_REL: &str = "knowledge/meta/graph.jsonl";
pub const HANDOFFS_DIR_REL: &str = "knowledge/meta/handoffs";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum NodeKind {
    WorldBible,
    Outline,
    FineOutlineBatch,
    ChapterBody,
    VolumeReview,
    Research,
    IntentRouter,
    SyncCanon,
    EnsureFineOutline,
    Custom,
}

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

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PlanNode {
    pub id: String,
    pub title: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub spec: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub spec_template: Option<String>,
    #[serde(default)]
    pub deps: Vec<String>,
    #[serde(default = "default_node_kind")]
    pub kind: NodeKind,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub artifacts: Vec<ArtifactRef>,
    #[serde(default)]
    pub acceptance: Acceptance,
    #[serde(default)]
    pub rollback: Rollback,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub iterate: Option<Iterate>,
}

fn default_node_kind() -> NodeKind {
    NodeKind::Custom
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct LoopCursor {
    #[serde(default = "one")]
    pub chapter: u32,
    #[serde(default = "one")]
    pub volume: u32,
    #[serde(default)]
    pub fine_outline_through: u32,
    #[serde(default = "one")]
    pub round: u32,
}

fn one() -> u32 {
    1
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LoopUntil {
    pub op: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub value_from: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub value: Option<u32>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LoopOnAdvance {
    pub reopen: Vec<String>,
    /// Clear `session_id` on reopen stations when the loop advances (default true).
    #[serde(default = "default_true")]
    pub clear_node_sessions: bool,
    /// Clear cached `effective_spec` so the next start re-renders NodeObjective (default true).
    #[serde(default = "default_true")]
    pub reinject_objectives: bool,
    /// When true (default), loop advance must not delete `world_state_board` files (KeepFiles).
    #[serde(default = "default_true")]
    pub preserve_canon_files: bool,
}

fn default_true() -> bool {
    true
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BookLoop {
    pub id: String,
    pub stations: Vec<String>,
    pub entry: String,
    pub advance_after: String,
    #[serde(default)]
    pub cursor: LoopCursor,
    pub until: LoopUntil,
    pub on_advance: LoopOnAdvance,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub world_state_board: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PlanGraph {
    pub version: String,
    #[serde(default)]
    pub nodes: Vec<PlanNode>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub loops: Vec<BookLoop>,
    #[serde(default)]
    pub enforce_gates: bool,
    #[serde(default = "default_max_parallel")]
    pub max_parallel_nodes: usize,
    #[serde(default)]
    pub target_chapters: Option<u32>,
    #[serde(default)]
    pub auto_start_ready: bool,
}

fn default_max_parallel() -> usize {
    4
}

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
    pub session_id: Option<String>,
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
    pub cursor: LoopCursor,
    #[serde(default)]
    pub phase: LoopPhase,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub active_station_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub paused_reason: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_advance_at: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct GraphSettings {
    #[serde(default)]
    pub enforce_gates: bool,
    #[serde(default = "default_max_parallel")]
    pub max_parallel_nodes: usize,
    #[serde(default)]
    pub auto_start_ready: bool,
    #[serde(default)]
    pub target_chapters: Option<u32>,
}

impl Default for GraphSettings {
    fn default() -> Self {
        Self {
            enforce_gates: false,
            max_parallel_nodes: default_max_parallel(),
            auto_start_ready: false,
            target_chapters: None,
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

/// IPC / UI snapshot DTOs
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
    pub kind: NodeKind,
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
    pub cursor: LoopCursor,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub target_chapters: Option<u32>,
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
    /// False when work has no formal plan-graph yet (interview / template-not-applied).
    #[serde(default = "default_true")]
    pub has_plan: bool,
}
