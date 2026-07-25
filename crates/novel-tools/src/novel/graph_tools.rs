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
                "no plan-graph.json — use GraphApplyTemplate or GraphCommitPlan first".into(),
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
            cb(a.loop_id.clone(), a.chapter, a.reset_node_ids.clone());
        }
    }
    let extra = adv
        .map(|a| format!(" Loop `{}` advanced to Ch.{}", a.loop_id, a.chapter))
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
        "Apply the bundled Book-Loop plan-graph template to this work (creates plan-graph.json + state). Use after the author agrees to start from the default skeleton. Set force=true only to replace an existing plan."
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
        let plan = novel_graph::parse_plan(&raw).map_err(|e| ToolError::Execution(e.to_string()))?;
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
    use novel_graph::{default_plan, ensure_graph_initialized, save_plan, NodeStatus};
    use tempfile::TempDir;

    fn ctx(tmp: &TempDir) -> ToolContext {
        ToolContext {
            permission_mode: PermissionMode::Auto,
            project_root: tmp.path().to_path_buf(),
            ..ToolContext::new(tmp.path().to_path_buf())
        }
    }

    async fn seeded(tmp: &TempDir) -> ToolContext {
        let plan = default_plan().unwrap();
        save_plan(tmp.path(), &plan).unwrap();
        let _ = ensure_graph_initialized(tmp.path()).unwrap();
        ctx(tmp)
    }

    #[tokio::test]
    async fn graph_query_operations() {
        let tmp = TempDir::new().unwrap();
        let c = seeded(&tmp).await;
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
        let c = seeded(&tmp).await;
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
        let c = seeded(&tmp).await;
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
        let mut c = seeded(&tmp).await;
        c.on_graph_loop_advanced = Some(Arc::new(move |loop_id, chapter, reset| {
            *saw2.lock().unwrap() = Some((loop_id, chapter, reset));
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
        let (loop_id, chapter, reset) = got.expect("on_graph_loop_advanced must fire");
        assert_eq!(loop_id, "book-body");
        assert_eq!(chapter, 2);
        assert!(reset.iter().any(|id| id == "sync-canon"));
    }

    #[tokio::test]
    async fn graph_submit_auditor_stays_verifying() {
        // ensure-fine-outline has machine: auditor + human: false → stays Verifying.
        let tmp = TempDir::new().unwrap();
        let c = seeded(&tmp).await;
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
        let mut c = seeded(&tmp).await;
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
}
