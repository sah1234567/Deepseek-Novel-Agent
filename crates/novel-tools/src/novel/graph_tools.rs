//! Graph tools: query / advance / submit / mark verified.

use crate::{require_str, Tool, ToolContext, ToolError, ToolOutput};
use async_trait::async_trait;
use novel_graph::{build_snapshot, GraphTracker};
use serde_json::{json, Value};

fn load_tracker(ctx: &ToolContext) -> Result<GraphTracker, ToolError> {
    GraphTracker::load(&ctx.project_root)
        .map_err(|e| ToolError::Execution(e.to_string()))?
        .ok_or_else(|| {
            ToolError::Execution(
                "no plan-graph.json — use GraphCommitPlan or PlanBuilder first".into(),
            )
        })
}

fn save_tracker(ctx: &ToolContext, t: &GraphTracker) -> Result<(), ToolError> {
    t.save(&ctx.project_root)
        .map_err(|e| ToolError::Execution(e.to_string()))?;
    // Notify UI (Event::GraphStateChanged) — same path as Write/Edit demote hooks.
    if let Some(cb) = &ctx.on_graph_state_changed {
        cb();
    }
    Ok(())
}

fn query_operation(t: &GraphTracker, op: &str, node_id: Option<&str>) -> Result<String, ToolError> {
    match op {
        "summary" => {
            let snap = build_snapshot(t);
            serde_json::to_string_pretty(&snap).map_err(|e| ToolError::Execution(e.to_string()))
        }
        "node" => {
            let id = node_id.ok_or_else(|| ToolError::Execution("node_id required".into()))?;
            let n = t
                .node_plan(id)
                .map_err(|e| ToolError::Execution(e.to_string()))?;
            let rt = t.state.nodes.get(id);
            let obj = t
                .node_objective_block(id)
                .map_err(|e| ToolError::Execution(e.to_string()))?;
            serde_json::to_string_pretty(&json!({
                "plan": n,
                "runtime": rt,
                "objective": obj,
            }))
            .map_err(|e| ToolError::Execution(e.to_string()))
        }
        "ready" => {
            let ids: Vec<_> = t
                .state
                .nodes
                .iter()
                .filter(|(_, r)| r.status == novel_graph::NodeStatus::Ready)
                .map(|(id, _)| id.clone())
                .collect();
            serde_json::to_string_pretty(&ids).map_err(|e| ToolError::Execution(e.to_string()))
        }
        "pending_approval" => {
            let ids: Vec<_> = t
                .state
                .nodes
                .iter()
                .filter(|(_, r)| r.status == novel_graph::NodeStatus::AwaitingApproval)
                .map(|(id, _)| id.clone())
                .collect();
            serde_json::to_string_pretty(&ids).map_err(|e| ToolError::Execution(e.to_string()))
        }
        _ => Err(ToolError::Execution(format!("unknown operation: {op}"))),
    }
}

pub struct GraphQueryTool;

#[async_trait]
impl Tool for GraphQueryTool {
    fn name(&self) -> &str {
        "GraphQuery"
    }
    fn description(&self) -> &str {
        "Query graph plan/runtime: summary | node | ready | pending_approval"
    }
    fn input_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "operation": {
                    "type": "string",
                    "enum": ["summary", "node", "ready", "pending_approval"]
                },
                "node_id": { "type": "string" }
            },
            "required": ["operation"]
        })
    }
    fn is_read_only(&self) -> bool {
        true
    }
    fn skips_normal_permission_ask(&self) -> bool {
        true
    }

    async fn call(&self, input: Value, ctx: &ToolContext) -> Result<ToolOutput, ToolError> {
        let op = require_str(&input, "operation")?;
        let t = load_tracker(ctx)?;
        let node_id = input.get("node_id").and_then(|v| v.as_str());
        let content = query_operation(&t, op.as_str(), node_id)?;
        Ok(ToolOutput {
            content,
            is_error: false,
        })
    }
}

pub struct GraphAdvanceTool;

#[async_trait]
impl Tool for GraphAdvanceTool {
    fn name(&self) -> &str {
        "GraphAdvance"
    }
    fn description(&self) -> &str {
        "Start a Ready graph node (Running) and set focus"
    }
    fn input_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "node_id": { "type": "string" }
            },
            "required": ["node_id"]
        })
    }
    fn is_read_only(&self) -> bool {
        false
    }

    async fn call(&self, input: Value, ctx: &ToolContext) -> Result<ToolOutput, ToolError> {
        let id = require_str(&input, "node_id")?;
        let mut t = load_tracker(ctx)?;
        t.start_node(&id, None)
            .map_err(|e| ToolError::Execution(e.to_string()))?;
        t.set_focus(Some(id.clone()))
            .map_err(|e| ToolError::Execution(e.to_string()))?;
        let obj = t
            .node_objective_block(&id)
            .map_err(|e| ToolError::Execution(e.to_string()))?;
        save_tracker(ctx, &t)?;
        Ok(ToolOutput {
            content: format!("Node `{id}` → Running.\n\n{obj}"),
            is_error: false,
        })
    }
}

fn auditor_gate_required(t: &GraphTracker, id: &str) -> Result<bool, ToolError> {
    let machine = t
        .node_plan(id)
        .map_err(|e| ToolError::Execution(e.to_string()))?
        .acceptance
        .machine;
    Ok(matches!(
        machine,
        novel_graph::MachineAcceptance::Auditor | novel_graph::MachineAcceptance::Verifier
    ))
}

fn submit_node(
    ctx: &ToolContext,
    t: &mut GraphTracker,
    id: &str,
    summary: String,
) -> Result<ToolOutput, ToolError> {
    t.set_pending_summary(id, summary)
        .map_err(|e| ToolError::Execution(e.to_string()))?;
    // Always run tracker exit gate (plan human OR human_intervened → AwaitingApproval).
    t.submit_for_approval(id)
        .map_err(|e| ToolError::Execution(e.to_string()))?;
    let status = t
        .state
        .nodes
        .get(id)
        .map(|rt| rt.status)
        .ok_or_else(|| ToolError::Execution(format!("node `{id}` missing after submit")))?;
    if matches!(status, novel_graph::NodeStatus::AwaitingApproval) {
        save_tracker(ctx, t)?;
        return Ok(ToolOutput {
            content: format!(
                "Node `{id}` → AwaitingApproval. Author must graph_approve / graph_reject."
            ),
            is_error: false,
        });
    }
    // Verifying: machine auditor/verifier stays; None may auto-achieve.
    if auditor_gate_required(t, id)? {
        save_tracker(ctx, t)?;
        return Ok(ToolOutput {
            content: format!(
                "Node `{id}` → Verifying. Audit evidence (InvokeSkill audit-* + AuditStatusUpdate) required before Achieved."
            ),
            is_error: false,
        });
    }
    let adv = t
        .approve(&ctx.project_root, id)
        .map_err(|e| ToolError::Execution(e.to_string()))?;
    save_tracker(ctx, t)?;
    if let Some(ref a) = adv {
        if let Some(cb) = &ctx.on_graph_loop_advanced {
            cb(
                a.loop_id.clone(),
                a.snapshot_key.clone(),
                a.counters.clone(),
                a.reset_node_ids.clone(),
            );
        }
    }
    let extra = adv
        .map(|a| format!(" Loop `{}` advanced to {}", a.loop_id, a.snapshot_key))
        .unwrap_or_default();
    Ok(ToolOutput {
        content: format!("Node `{id}` → Achieved.{extra}"),
        is_error: false,
    })
}

pub struct GraphSubmitForApprovalTool;

#[async_trait]
impl Tool for GraphSubmitForApprovalTool {
    fn name(&self) -> &str {
        "GraphSubmitForApproval"
    }
    fn description(&self) -> &str {
        "Submit current node wrap-up summary for approval / Achieved. summary = non-CoT final text."
    }
    fn input_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "node_id": { "type": "string" },
                "summary": { "type": "string", "description": "Non-CoT wrap-up of what was done" }
            },
            "required": ["node_id", "summary"]
        })
    }
    fn is_read_only(&self) -> bool {
        false
    }

    async fn call(&self, input: Value, ctx: &ToolContext) -> Result<ToolOutput, ToolError> {
        let id = require_str(&input, "node_id")?;
        let summary = require_str(&input, "summary")?;
        let mut t = load_tracker(ctx)?;
        submit_node(ctx, &mut t, &id, summary.to_string())
    }
}

pub struct GraphReopenTool;

#[async_trait]
impl Tool for GraphReopenTool {
    fn name(&self) -> &str {
        "GraphReopen"
    }
    fn description(&self) -> &str {
        "Reopen an Achieved graph node (REGATE); optional cascade to downstream nodes"
    }
    fn input_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "node_id": { "type": "string" },
                "cascade_downstream": {
                    "type": "boolean",
                    "description": "Demote downstream achieved nodes to Waiting (default true)"
                },
                "note": {
                    "type": "string",
                    "description": "Optional REGATE: <id> / REASON: text (fail-closed if id mismatches)"
                }
            },
            "required": ["node_id"]
        })
    }
    fn is_read_only(&self) -> bool {
        false
    }

    async fn call(&self, input: Value, ctx: &ToolContext) -> Result<ToolOutput, ToolError> {
        let id = require_str(&input, "node_id")?;
        let cascade = input
            .get("cascade_downstream")
            .and_then(|v| v.as_bool())
            .unwrap_or(true);
        if let Some(note) = input.get("note").and_then(|v| v.as_str()) {
            if let Some((target, _)) = novel_graph::parse_regate_directive(note) {
                if target != id {
                    return Err(ToolError::Execution(format!(
                        "REGATE target `{target}` does not match node_id `{id}`"
                    )));
                }
            }
        }
        let mut t = load_tracker(ctx)?;
        let demoted = t
            .reopen(&id, cascade)
            .map_err(|e| ToolError::Execution(e.to_string()))?;
        save_tracker(ctx, &t)?;
        Ok(ToolOutput {
            content: format!(
                "Reopened `{id}` (cascade={cascade}). Demoted: {}",
                demoted.join(", ")
            ),
            is_error: false,
        })
    }
}

pub struct GraphMarkVerifiedTool;

#[async_trait]
impl Tool for GraphMarkVerifiedTool {
    fn name(&self) -> &str {
        "GraphMarkVerified"
    }
    fn description(&self) -> &str {
        "Mark machine/auditor verification done (Verifying)"
    }
    fn input_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": { "node_id": { "type": "string" } },
            "required": ["node_id"]
        })
    }
    fn is_read_only(&self) -> bool {
        false
    }

    async fn call(&self, input: Value, ctx: &ToolContext) -> Result<ToolOutput, ToolError> {
        let id = require_str(&input, "node_id")?;
        let mut t = load_tracker(ctx)?;
        t.mark_verifying(&id)
            .map_err(|e| ToolError::Execution(e.to_string()))?;
        save_tracker(ctx, &t)?;
        Ok(ToolOutput {
            content: format!("Node `{id}` → Verifying"),
            is_error: false,
        })
    }
}

/// Apply the bundled default plan skeleton (no-op if a formal plan already exists, unless force).
pub struct GraphApplyTemplateTool;

#[async_trait]
impl Tool for GraphApplyTemplateTool {
    fn name(&self) -> &str {
        "GraphApplyTemplate"
    }
    fn description(&self) -> &str {
        "[DEPRECATED — use PlanBuilder instead] Write an empty plan-graph skeleton (no nodes/loops). After applying, use PlanBuilder to incrementally construct the workflow."
    }
    fn input_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "force": {
                    "type": "boolean",
                    "description": "Replace existing plan-graph.json (destructive). Default false."
                }
            }
        })
    }
    fn is_read_only(&self) -> bool {
        false
    }

    async fn call(&self, input: Value, ctx: &ToolContext) -> Result<ToolOutput, ToolError> {
        let force = input
            .get("force")
            .and_then(|v| v.as_bool())
            .unwrap_or(false);
        if novel_graph::plan_exists(&ctx.project_root) && !force {
            return Ok(ToolOutput {
                content: "plan-graph.json already exists — pass force=true to replace, or use GraphCommitPlan with a custom JSON.".into(),
                is_error: false,
            });
        }
        if force && novel_graph::plan_exists(&ctx.project_root) {
            let p = novel_graph::plan_path(&ctx.project_root);
            std::fs::remove_file(&p).map_err(|e| ToolError::Execution(e.to_string()))?;
            let sp = novel_graph::state_path(&ctx.project_root);
            let _ = std::fs::remove_file(sp);
        }
        novel_graph::write_default_plan_file(&ctx.project_root)
            .map_err(|e| ToolError::Execution(e.to_string()))?;
        if let Some(cb) = &ctx.on_graph_plan_committed {
            cb();
        }
        Ok(ToolOutput {
            content: "Applied default plan-graph template. Open Graph to review nodes; then Start Ready stations.".into(),
            is_error: false,
        })
    }
}

/// Validate and commit a plan-graph JSON document as the formal work plan.
pub struct GraphCommitPlanTool;

#[async_trait]
impl Tool for GraphCommitPlanTool {
    fn name(&self) -> &str {
        "GraphCommitPlan"
    }
    fn description(&self) -> &str {
        "Commit a plan-graph JSON as knowledge/meta/plan-graph.json and initialize graph-state. Validates DAG before write."
    }
    fn input_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "plan_json": {
                    "type": "string",
                    "description": "Full plan-graph JSON document"
                },
                "replace": {
                    "type": "boolean",
                    "description": "Allow overwrite if plan already exists. Default false."
                }
            },
            "required": ["plan_json"]
        })
    }
    fn is_read_only(&self) -> bool {
        false
    }

    async fn call(&self, input: Value, ctx: &ToolContext) -> Result<ToolOutput, ToolError> {
        let raw = require_str(&input, "plan_json")?;
        let replace = input
            .get("replace")
            .and_then(|v| v.as_bool())
            .unwrap_or(false);
        if novel_graph::plan_exists(&ctx.project_root) && !replace {
            return Err(ToolError::Execution(
                "plan-graph.json already exists — pass replace=true to overwrite".into(),
            ));
        }
        let plan =
            novel_graph::parse_plan(&raw).map_err(|e| ToolError::Execution(e.to_string()))?;
        novel_graph::save_plan(&ctx.project_root, &plan)
            .map_err(|e| ToolError::Execution(e.to_string()))?;
        let t = novel_graph::GraphTracker::new(plan);
        t.save(&ctx.project_root)
            .map_err(|e| ToolError::Execution(e.to_string()))?;
        if let Some(cb) = &ctx.on_graph_plan_committed {
            cb();
        }
        Ok(ToolOutput {
            content: format!(
                "Committed plan-graph ({} nodes, {} loops). Open Graph once to review.",
                t.plan.nodes.len(),
                t.plan.loops.len()
            ),
            is_error: false,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::PermissionMode;
    use novel_graph::{
        ensure_graph_initialized, save_plan, Acceptance, AdvanceRule, CounterOp, Cursor,
        LoopOnAdvance, MachineAcceptance, NodeStatus, Until, WorkflowLoop,
    };
    use std::collections::HashMap;
    use std::sync::Arc;
    use tempfile::TempDir;

    fn ctx(tmp: &TempDir) -> ToolContext {
        ToolContext {
            permission_mode: PermissionMode::Auto,
            project_root: tmp.path().to_path_buf(),
            ..ToolContext::new(tmp.path().to_path_buf())
        }
    }

    /// Build a classic 6-node book-loop plan (matches coverage_tests::classic_plan).
    fn classic_plan() -> novel_graph::PlanGraph {
        use novel_graph::{
            ArtifactRef, ArtifactRole, HumanGate, Iterate, IterateUntil, OnReject, PlanGraph,
            PlanNode,
        };
        PlanGraph {
            version: "1".into(),
            enforce_gates: true,
            max_parallel_nodes: 4,
            auto_start_ready: false,
            settings: {
                let mut s = HashMap::new();
                s.insert("targetChapters".into(), serde_json::json!(200));
                s
            },
            nodes: vec![
                PlanNode {
                    id: "world-bible".into(),
                    title: "世界观".into(),
                    spec: Some("build world bible".into()),
                    tags: vec!["world_bible".into()],
                    artifacts: vec![ArtifactRef {
                        path: Some("knowledge/shared-systems/背景设定.md".into()),
                        role: ArtifactRole::PrimaryDeliverable,
                        path_template: None,
                    }],
                    acceptance: Acceptance {
                        machine: MachineAcceptance::None,
                        human: Some(HumanGate {
                            required: true,
                            on_reject: OnReject::Continue,
                            prompt: Some("批准世界观？".into()),
                            review: vec![],
                        }),
                    },
                    ..Default::default()
                },
                PlanNode {
                    id: "outline".into(),
                    title: "大纲".into(),
                    spec: Some("write outline".into()),
                    deps: vec!["world-bible".into()],
                    tags: vec!["outline".into()],
                    artifacts: vec![ArtifactRef {
                        path: Some("knowledge/plot/大纲.md".into()),
                        role: ArtifactRole::PrimaryDeliverable,
                        path_template: None,
                    }],
                    acceptance: Acceptance {
                        machine: MachineAcceptance::Auditor,
                        human: Some(HumanGate {
                            required: true,
                            on_reject: OnReject::Continue,
                            prompt: Some("批准大纲？".into()),
                            review: vec![],
                        }),
                    },
                    ..Default::default()
                },
                PlanNode {
                    id: "ensure-fine-outline".into(),
                    title: "细纲确保".into(),
                    spec_template: Some("fine outline ch{{cursor.chapter}}".into()),
                    deps: vec!["outline".into()],
                    tags: vec!["ensure_fine_outline".into()],
                    artifacts: vec![ArtifactRef {
                        path_template: Some(
                            "knowledge/plot/细纲/chapter-{{cursor.chapter | pad3}}-细纲.md".into(),
                        ),
                        role: ArtifactRole::PrimaryDeliverable,
                        path: None,
                    }],
                    acceptance: Acceptance {
                        machine: MachineAcceptance::Auditor,
                        human: Some(HumanGate {
                            required: false,
                            on_reject: OnReject::Continue,
                            prompt: None,
                            review: vec![],
                        }),
                    },
                    ..Default::default()
                },
                PlanNode {
                    id: "write-chapter".into(),
                    title: "写章".into(),
                    spec_template: Some("write chapter {{cursor.chapter}}".into()),
                    deps: vec!["ensure-fine-outline".into()],
                    tags: vec!["chapter_body".into()],
                    artifacts: vec![ArtifactRef {
                        path_template: Some("chapters/chapter-{{cursor.chapter | pad3}}.md".into()),
                        role: ArtifactRole::PrimaryDeliverable,
                        path: None,
                    }],
                    iterate: Some(Iterate {
                        max_iterations: 8,
                        until: IterateUntil::AuditorPass,
                    }),
                    ..Default::default()
                },
                PlanNode {
                    id: "sync-canon".into(),
                    title: "正典同步".into(),
                    spec_template: Some("sync canon after ch{{cursor.chapter}}".into()),
                    deps: vec!["write-chapter".into()],
                    tags: vec!["sync_canon".into()],
                    artifacts: vec![
                        ArtifactRef {
                            path: Some("knowledge/characters/".into()),
                            role: ArtifactRole::Aux,
                            path_template: None,
                        },
                        ArtifactRef {
                            path: Some("knowledge/INDEX.md".into()),
                            role: ArtifactRole::Aux,
                            path_template: None,
                        },
                    ],
                    ..Default::default()
                },
                PlanNode {
                    id: "volume-review".into(),
                    title: "卷审".into(),
                    spec: Some("volume review".into()),
                    deps: vec!["sync-canon".into()],
                    tags: vec!["volume_review".into()],
                    ..Default::default()
                },
            ],
            loops: vec![WorkflowLoop {
                id: "book-body".into(),
                stations: vec![
                    "ensure-fine-outline".into(),
                    "write-chapter".into(),
                    "sync-canon".into(),
                ],
                entry: "ensure-fine-outline".into(),
                advance_after: "sync-canon".into(),
                cursor: Cursor {
                    counters: {
                        let mut c = HashMap::new();
                        c.insert("chapter".into(), 1);
                        c.insert("volume".into(), 1);
                        c.insert("round".into(), 1);
                        c
                    },
                    tags: HashMap::new(),
                },
                advance: AdvanceRule {
                    increment: "chapter".into(),
                    step: 1,
                    side_effects: vec![CounterOp::Increment {
                        counter: "round".into(),
                        by: 1,
                    }],
                },
                until: Until::CounterGt {
                    counter: "chapter".into(),
                    value: None,
                    value_from: Some("work_meta.settings.targetChapters".into()),
                },
                on_advance: LoopOnAdvance {
                    reopen: vec![
                        "ensure-fine-outline".into(),
                        "write-chapter".into(),
                        "sync-canon".into(),
                    ],
                    clear_node_sessions: true,
                    reinject_objectives: true,
                    preserve_canon_files: true,
                },
                world_state_board: vec![
                    "knowledge/characters/".into(),
                    "knowledge/INDEX.md".into(),
                ],
            }],
        }
    }

    async fn seeded_with_classic_plan(tmp: &TempDir) -> ToolContext {
        let plan = classic_plan();
        save_plan(tmp.path(), &plan).unwrap();
        let _ = ensure_graph_initialized(tmp.path()).unwrap();
        ctx(tmp)
    }

    #[tokio::test]
    async fn graph_query_operations() {
        let tmp = TempDir::new().unwrap();
        let c = seeded_with_classic_plan(&tmp).await;
        for op in ["summary", "ready", "pending_approval"] {
            let out = GraphQueryTool
                .call(json!({"operation": op}), &c)
                .await
                .unwrap();
            assert!(!out.is_error);
        }
        let out = GraphQueryTool
            .call(json!({"operation": "node", "node_id": "world-bible"}), &c)
            .await
            .unwrap();
        assert!(out.content.contains("world-bible"));
        assert!(GraphQueryTool
            .call(json!({"operation": "nope"}), &c)
            .await
            .is_err());
    }

    #[tokio::test]
    async fn graph_advance_submit_mark_reopen() {
        let tmp = TempDir::new().unwrap();
        let c = seeded_with_classic_plan(&tmp).await;
        GraphAdvanceTool
            .call(json!({"node_id": "world-bible"}), &c)
            .await
            .unwrap();
        let out = GraphSubmitForApprovalTool
            .call(
                json!({"node_id": "world-bible", "summary": "created bible files"}),
                &c,
            )
            .await
            .unwrap();
        assert!(out.content.contains("AwaitingApproval") || out.content.contains("Achieved"));
        // mark verifying on outline after forcing ready
        {
            let mut t = GraphTracker::load(tmp.path()).unwrap().unwrap();
            t.state.nodes.get_mut("world-bible").unwrap().status = NodeStatus::Achieved;
            t.recompute_ready();
            t.start_node("outline", None).unwrap();
            t.save(tmp.path()).unwrap();
        }
        GraphMarkVerifiedTool
            .call(json!({"node_id": "outline"}), &c)
            .await
            .unwrap();
        {
            let mut t = GraphTracker::load(tmp.path()).unwrap().unwrap();
            t.state.nodes.get_mut("outline").unwrap().status = NodeStatus::Achieved;
            t.state.nodes.get_mut("outline").unwrap().pending_summary = Some("ok".into());
            t.save(tmp.path()).unwrap();
        }
        GraphReopenTool
            .call(
                json!({
                    "node_id": "outline",
                    "cascade_downstream": true,
                    "note": "REGATE: outline\nREASON: fix outline"
                }),
                &c,
            )
            .await
            .unwrap();
        assert!(GraphReopenTool
            .call(
                json!({
                    "node_id": "outline",
                    "note": "REGATE: world-bible\nREASON: wrong"
                }),
                &c
            )
            .await
            .is_err());
    }

    #[tokio::test]
    async fn graph_submit_auto_achieves_machine_none() {
        // sync-canon has machine: none + human: false → auto-Achieve.
        let tmp = TempDir::new().unwrap();
        let c = seeded_with_classic_plan(&tmp).await;
        {
            let mut t = GraphTracker::load(tmp.path()).unwrap().unwrap();
            for id in [
                "world-bible",
                "outline",
                "ensure-fine-outline",
                "write-chapter",
            ] {
                t.state.nodes.get_mut(id).unwrap().status = NodeStatus::Achieved;
                t.state.nodes.get_mut(id).unwrap().pending_summary = Some("ok".into());
            }
            t.recompute_ready();
            t.start_node("sync-canon", None).unwrap();
            t.record_file_touch("sync-canon", "knowledge/characters/hero.md", "update")
                .unwrap();
            t.save(tmp.path()).unwrap();
        }
        let out = GraphSubmitForApprovalTool
            .call(
                json!({
                    "node_id": "sync-canon",
                    "summary": "canon updated for ch1"
                }),
                &c,
            )
            .await
            .unwrap();
        assert!(out.content.contains("Achieved"));
    }

    #[tokio::test]
    async fn graph_submit_loop_advance_notifies_callback() {
        use std::sync::{Arc, Mutex};

        let tmp = TempDir::new().unwrap();
        let saw = Arc::new(Mutex::new(None));
        let saw2 = Arc::clone(&saw);
        let mut c = seeded_with_classic_plan(&tmp).await;
        c.on_graph_loop_advanced = Some(Arc::new(move |loop_id, snapshot_key, counters, reset| {
            *saw2.lock().unwrap() = Some((loop_id, snapshot_key, counters, reset));
        }));
        {
            let mut t = GraphTracker::load(tmp.path()).unwrap().unwrap();
            for id in [
                "world-bible",
                "outline",
                "ensure-fine-outline",
                "write-chapter",
            ] {
                t.state.nodes.get_mut(id).unwrap().status = NodeStatus::Achieved;
                t.state.nodes.get_mut(id).unwrap().pending_summary = Some("ok".into());
            }
            t.recompute_ready();
            t.start_node("sync-canon", None).unwrap();
            t.record_file_touch("sync-canon", "knowledge/characters/hero.md", "update")
                .unwrap();
            t.save(tmp.path()).unwrap();
        }
        let out = GraphSubmitForApprovalTool
            .call(
                json!({
                    "node_id": "sync-canon",
                    "summary": "canon updated for ch1"
                }),
                &c,
            )
            .await
            .unwrap();
        assert!(out.content.contains("advanced") || out.content.contains("Achieved"));
        let got = saw.lock().unwrap().clone();
        let (loop_id, snapshot_key, _counters, reset) =
            got.expect("on_graph_loop_advanced must fire");
        assert_eq!(loop_id, "book-body");
        assert_eq!(snapshot_key, "chapter=2");
        assert!(reset.iter().any(|id| id == "sync-canon"));
    }

    #[tokio::test]
    async fn graph_submit_auditor_stays_verifying() {
        // ensure-fine-outline has machine: auditor + human: false → stays Verifying.
        let tmp = TempDir::new().unwrap();
        let c = seeded_with_classic_plan(&tmp).await;
        {
            let mut t = GraphTracker::load(tmp.path()).unwrap().unwrap();
            t.state.nodes.get_mut("world-bible").unwrap().status = NodeStatus::Achieved;
            t.state.nodes.get_mut("outline").unwrap().status = NodeStatus::Achieved;
            t.state
                .nodes
                .get_mut("world-bible")
                .unwrap()
                .pending_summary = Some("ok".into());
            t.state.nodes.get_mut("outline").unwrap().pending_summary = Some("ok".into());
            t.recompute_ready();
            t.start_node("ensure-fine-outline", None).unwrap();
            t.record_file_touch(
                "ensure-fine-outline",
                "knowledge/plot/细纲/chapter-001-细纲.md",
                "create",
            )
            .unwrap();
            t.save(tmp.path()).unwrap();
        }
        let out = GraphSubmitForApprovalTool
            .call(
                json!({
                    "node_id": "ensure-fine-outline",
                    "summary": "fine outline for ch1 ready"
                }),
                &c,
            )
            .await
            .unwrap();
        assert!(
            out.content.contains("Verifying"),
            "auditor-gated node should stay Verifying, got: {}",
            out.content
        );
    }

    #[tokio::test]
    async fn graph_submit_human_intervened_forces_awaiting_approval() {
        use std::sync::atomic::{AtomicBool, Ordering};
        use std::sync::Arc;

        let tmp = TempDir::new().unwrap();
        let flag = Arc::new(AtomicBool::new(false));
        let flag2 = Arc::clone(&flag);
        let mut c = seeded_with_classic_plan(&tmp).await;
        c.on_graph_state_changed = Some(Arc::new(move || {
            flag2.store(true, Ordering::SeqCst);
        }));
        {
            let mut t = GraphTracker::load(tmp.path()).unwrap().unwrap();
            t.state.nodes.get_mut("world-bible").unwrap().status = NodeStatus::Achieved;
            t.state.nodes.get_mut("outline").unwrap().status = NodeStatus::Achieved;
            t.recompute_ready();
            t.start_node("ensure-fine-outline", None).unwrap();
            t.mark_human_intervened("ensure-fine-outline").unwrap();
            t.save(tmp.path()).unwrap();
        }
        let out = GraphSubmitForApprovalTool
            .call(
                json!({
                    "node_id": "ensure-fine-outline",
                    "summary": "author edited fine outline"
                }),
                &c,
            )
            .await
            .unwrap();
        assert!(
            out.content.contains("AwaitingApproval"),
            "human_intervened must force approval even if plan human=false, got: {}",
            out.content
        );
        assert!(
            flag.load(Ordering::SeqCst),
            "save_tracker must notify on_graph_state_changed"
        );
    }

    // GraphApplyTemplate / GraphCommitPlan boundaries:
    // - apply: no plan → write; exists without force → soft message; force → replace + callback
    // - commit: success + callback; exists without replace → err; invalid JSON → err; replace ok
    fn minimal_valid_plan_json() -> String {
        r#"{
          "version":"1",
          "nodes":[{"id":"a","title":"A","spec":"do a","artifacts":[{"path":"a.md","role":"primary_deliverable"}]}],
          "loops":[]
        }"#
        .into()
    }

    #[tokio::test]
    async fn graph_apply_template_create_skip_and_force() {
        use std::sync::atomic::{AtomicBool, Ordering};
        let tmp = TempDir::new().unwrap();
        let c = ctx(&tmp);
        let out = GraphApplyTemplateTool.call(json!({}), &c).await.unwrap();
        assert!(out.content.contains("Applied"));
        assert!(novel_graph::plan_exists(tmp.path()));

        let skip = GraphApplyTemplateTool.call(json!({}), &c).await.unwrap();
        assert!(skip.content.contains("already exists"));

        let flag = Arc::new(AtomicBool::new(false));
        let flag2 = Arc::clone(&flag);
        let mut c2 = ctx(&tmp);
        c2.on_graph_plan_committed = Some(Arc::new(move || {
            flag2.store(true, Ordering::SeqCst);
        }));
        let forced = GraphApplyTemplateTool
            .call(json!({"force": true}), &c2)
            .await
            .unwrap();
        assert!(forced.content.contains("Applied"));
        assert!(flag.load(Ordering::SeqCst));
        assert_eq!(GraphApplyTemplateTool.name(), "GraphApplyTemplate");
        assert!(!GraphApplyTemplateTool.is_read_only());
    }

    #[tokio::test]
    async fn graph_commit_plan_success_replace_and_errors() {
        use std::sync::atomic::{AtomicBool, Ordering};
        let tmp = TempDir::new().unwrap();
        let flag = Arc::new(AtomicBool::new(false));
        let flag2 = Arc::clone(&flag);
        let mut c = ctx(&tmp);
        c.on_graph_plan_committed = Some(Arc::new(move || {
            flag2.store(true, Ordering::SeqCst);
        }));

        let out = GraphCommitPlanTool
            .call(json!({"plan_json": minimal_valid_plan_json()}), &c)
            .await
            .unwrap();
        assert!(out.content.contains("Committed"));
        assert!(flag.load(Ordering::SeqCst));
        assert!(novel_graph::plan_exists(tmp.path()));

        let err = GraphCommitPlanTool
            .call(json!({"plan_json": minimal_valid_plan_json()}), &c)
            .await
            .unwrap_err();
        assert!(err.to_string().contains("already exists"));

        let replaced = GraphCommitPlanTool
            .call(
                json!({
                    "plan_json": minimal_valid_plan_json(),
                    "replace": true
                }),
                &c,
            )
            .await
            .unwrap();
        assert!(replaced.content.contains("Committed"));

        let bad = GraphCommitPlanTool
            .call(
                json!({"plan_json": "{\"version\":\"1\",\"nodes\":[]}", "replace": true}),
                &c,
            )
            .await
            .unwrap_err();
        assert!(bad.to_string().contains("no nodes") || bad.to_string().contains("Validation"));
        assert_eq!(GraphCommitPlanTool.name(), "GraphCommitPlan");
        assert!(!GraphCommitPlanTool.is_read_only());
        let _ = GraphCommitPlanTool.input_schema();
    }
}
