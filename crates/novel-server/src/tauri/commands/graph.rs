//! Graph IPC commands (plan snapshot, approve/reject, loop controls).

use crate::tauri::engine_loop::EngineCommand;
use crate::tauri::graph_emit::{emit_graph_loop_advanced, emit_graph_loops_only, emit_graph_state};
use crate::tauri::state::CommandContext;
use novel_graph::{build_snapshot, empty_snapshot, GraphStateSnapshot, GraphTracker};
use serde::Serialize;
use tauri::Emitter;

use super::engine_ipc::{emit_interaction_mode_changed, send_engine_reply};

async fn work_root(ctx: &CommandContext) -> std::path::PathBuf {
    ctx.config.read().await.active_project.clone()
}

fn load(ctx_root: &std::path::Path) -> Result<GraphTracker, String> {
    GraphTracker::load(ctx_root)
        .map_err(|e| e.to_string())?
        .ok_or_else(|| "no plan-graph.json — use GraphCommitPlan or PlanBuilder first".into())
}

fn emit_loop_changed(
    ctx: &CommandContext,
    snapshot: &GraphStateSnapshot,
    advanced: &Option<LoopAdvancedPayload>,
) {
    if let Some(adv) = advanced {
        emit_graph_loop_advanced(
            &ctx.app_handle,
            &adv.loop_id,
            &adv.snapshot_key,
            &adv.counters,
            &adv.reset_node_ids,
            snapshot,
        );
    } else {
        emit_graph_loops_only(&ctx.app_handle, snapshot);
    }
}

fn emit_after_mutate(
    ctx: &CommandContext,
    snapshot: &GraphStateSnapshot,
    loop_advanced: &Option<LoopAdvancedPayload>,
) {
    emit_graph_state(&ctx.app_handle, snapshot);
    emit_loop_changed(ctx, snapshot, loop_advanced);
}

pub async fn graph_get_state(ctx: &CommandContext) -> Result<GraphStateSnapshot, String> {
    let root = work_root(ctx).await;
    match GraphTracker::load(&root) {
        Ok(Some(t)) => Ok(build_snapshot(&t)),
        Ok(None) => Ok(empty_snapshot()),
        // Legacy / corrupt plan must not fail IPC (would paint ErrorBanner on every boot).
        // Author can migrate via PlanBuilder; Graph UI shows empty until then.
        Err(e) => {
            tracing::warn!(error = %e, "graph_get_state: plan load failed; empty snapshot");
            Ok(empty_snapshot())
        }
    }
}

pub async fn graph_get_node(
    ctx: &CommandContext,
    node_id: String,
) -> Result<serde_json::Value, String> {
    let root = work_root(ctx).await;
    let t = load(&root)?;
    let n = t.node_plan(&node_id).map_err(|e| e.to_string())?;
    let rt = t.state.nodes.get(&node_id);
    let objective = t
        .node_objective_block(&node_id)
        .map_err(|e| e.to_string())?;
    Ok(serde_json::json!({
        "plan": n,
        "runtime": rt,
        "objective": objective,
    }))
}

pub async fn graph_get_loop(
    ctx: &CommandContext,
    loop_id: String,
) -> Result<serde_json::Value, String> {
    let root = work_root(ctx).await;
    let t = load(&root)?;
    let snap = build_snapshot(&t);
    snap.loops
        .into_iter()
        .find(|l| l.loop_id == loop_id)
        .map(serde_json::to_value)
        .transpose()
        .map_err(|e| e.to_string())?
        .ok_or_else(|| format!("loop not found: {loop_id}"))
}

pub async fn graph_activate_node(ctx: &CommandContext, node_id: String) -> Result<(), String> {
    let root = work_root(ctx).await;
    let mut t = load(&root)?;
    t.set_focus(Some(node_id)).map_err(|e| e.to_string())?;
    t.save(&root).map_err(|e| e.to_string())?;
    emit_graph_state(&ctx.app_handle, &build_snapshot(&t));
    send_engine_reply(ctx, |reply| EngineCommand::MarkInteractionWork { reply }).await?;
    emit_interaction_mode_changed(ctx, "work");
    Ok(())
}

pub async fn graph_clear_focus(ctx: &CommandContext) -> Result<(), String> {
    let root = work_root(ctx).await;
    if let Ok(Some(mut t)) = GraphTracker::load(&root) {
        t.set_focus(None).map_err(|e| e.to_string())?;
        t.save(&root).map_err(|e| e.to_string())?;
        emit_graph_state(&ctx.app_handle, &build_snapshot(&t));
    }
    send_engine_reply(ctx, |reply| EngineCommand::SetInteractionMode {
        mode: "orchestrate".into(),
        reply,
    })
    .await?;
    emit_interaction_mode_changed(ctx, "orchestrate");
    Ok(())
}

pub async fn graph_start_node(ctx: &CommandContext, node_id: String) -> Result<(), String> {
    let root = work_root(ctx).await;
    let mut t = load(&root)?;
    t.start_node(&node_id, None).map_err(|e| e.to_string())?;
    t.set_focus(Some(node_id)).map_err(|e| e.to_string())?;
    t.save(&root).map_err(|e| e.to_string())?;
    emit_graph_state(&ctx.app_handle, &build_snapshot(&t));
    send_engine_reply(ctx, |reply| EngineCommand::MarkInteractionWork { reply }).await?;
    emit_interaction_mode_changed(ctx, "work");
    Ok(())
}

#[derive(Debug, Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct GraphMutateResult {
    pub snapshot: GraphStateSnapshot,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub loop_advanced: Option<LoopAdvancedPayload>,
}

#[derive(Debug, Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct LoopAdvancedPayload {
    pub loop_id: String,
    pub snapshot_key: String,
    pub counters: std::collections::HashMap<String, i64>,
    pub reset_node_ids: Vec<String>,
}

pub async fn graph_approve(
    ctx: &CommandContext,
    node_id: String,
) -> Result<GraphMutateResult, String> {
    let root = work_root(ctx).await;
    let mut t = load(&root)?;
    let adv = t.approve(&root, &node_id).map_err(|e| e.to_string())?;
    t.save(&root).map_err(|e| e.to_string())?;
    let loop_advanced = adv.map(|a| LoopAdvancedPayload {
        loop_id: a.loop_id.clone(),
        snapshot_key: a.snapshot_key.clone(),
        counters: a.counters.clone(),
        reset_node_ids: a.reset_node_ids,
    });
    let snapshot = build_snapshot(&t);
    emit_after_mutate(ctx, &snapshot, &loop_advanced);
    Ok(GraphMutateResult {
        snapshot,
        loop_advanced,
    })
}

pub async fn graph_reject(
    ctx: &CommandContext,
    node_id: String,
    note: String,
) -> Result<GraphMutateResult, String> {
    let root = work_root(ctx).await;
    let mut t = load(&root)?;
    t.reject(&node_id, &note).map_err(|e| e.to_string())?;
    t.save(&root).map_err(|e| e.to_string())?;
    let snapshot = build_snapshot(&t);
    emit_after_mutate(ctx, &snapshot, &None);
    Ok(GraphMutateResult {
        snapshot,
        loop_advanced: None,
    })
}

pub async fn graph_reopen(
    ctx: &CommandContext,
    node_id: String,
    cascade_downstream: Option<bool>,
) -> Result<GraphMutateResult, String> {
    let root = work_root(ctx).await;
    let mut t = load(&root)?;
    t.reopen(&node_id, cascade_downstream.unwrap_or(true))
        .map_err(|e| e.to_string())?;
    t.save(&root).map_err(|e| e.to_string())?;
    let snapshot = build_snapshot(&t);
    emit_after_mutate(ctx, &snapshot, &None);
    Ok(GraphMutateResult {
        snapshot,
        loop_advanced: None,
    })
}

pub async fn graph_loop_pause(
    ctx: &CommandContext,
    loop_id: String,
    reason: Option<String>,
) -> Result<GraphMutateResult, String> {
    let root = work_root(ctx).await;
    let mut t = load(&root)?;
    t.pause_loop(&loop_id, reason.as_deref().unwrap_or("user"))
        .map_err(|e| e.to_string())?;
    t.save(&root).map_err(|e| e.to_string())?;
    let snapshot = build_snapshot(&t);
    emit_after_mutate(ctx, &snapshot, &None);
    Ok(GraphMutateResult {
        snapshot,
        loop_advanced: None,
    })
}

pub async fn graph_loop_resume(
    ctx: &CommandContext,
    loop_id: String,
) -> Result<GraphMutateResult, String> {
    let root = work_root(ctx).await;
    let mut t = load(&root)?;
    t.resume_loop(&loop_id).map_err(|e| e.to_string())?;
    t.save(&root).map_err(|e| e.to_string())?;
    let snapshot = build_snapshot(&t);
    emit_after_mutate(ctx, &snapshot, &None);
    Ok(GraphMutateResult {
        snapshot,
        loop_advanced: None,
    })
}

pub async fn graph_loop_set_target(
    ctx: &CommandContext,
    loop_id: String,
    key: String,
    value: serde_json::Value,
) -> Result<GraphMutateResult, String> {
    let root = work_root(ctx).await;
    let mut t = load(&root)?;
    t.set_loop_setting(&loop_id, &key, value)
        .map_err(|e| e.to_string())?;
    t.save(&root).map_err(|e| e.to_string())?;
    let snapshot = build_snapshot(&t);
    emit_after_mutate(ctx, &snapshot, &None);
    Ok(GraphMutateResult {
        snapshot,
        loop_advanced: None,
    })
}

pub async fn graph_loop_set_cursor(
    ctx: &CommandContext,
    loop_id: String,
    counters: std::collections::HashMap<String, i64>,
) -> Result<GraphMutateResult, String> {
    let root = work_root(ctx).await;
    let mut t = load(&root)?;
    t.set_loop_cursor(&loop_id, counters)
        .map_err(|e| e.to_string())?;
    t.save(&root).map_err(|e| e.to_string())?;
    let snapshot = build_snapshot(&t);
    emit_after_mutate(ctx, &snapshot, &None);
    Ok(GraphMutateResult {
        snapshot,
        loop_advanced: None,
    })
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LoopHistoryRow {
    pub snapshot_key: String,
    pub artifact_path: String,
    pub handoff_summary_preview: String,
    pub achieved_at: Option<String>,
}

pub async fn graph_loop_list_history(
    ctx: &CommandContext,
    loop_id: String,
    limit: Option<u32>,
) -> Result<Vec<LoopHistoryRow>, String> {
    let root = work_root(ctx).await;
    let t = load(&root)?;
    let lim = limit.unwrap_or(50) as usize;

    // Find which stations belong to this loop.
    let lp_plan = t
        .plan
        .loops
        .iter()
        .find(|l| l.id == loop_id)
        .ok_or_else(|| format!("loop not found: {loop_id}"))?;

    let mut rows = Vec::new();

    // Read snapshot-archived handoffs for each station.
    for station_id in &lp_plan.stations {
        if let Ok(archived) = novel_graph::list_handoff_snapshots(&root, station_id, lim) {
            for (snap_key, h) in archived {
                rows.push(LoopHistoryRow {
                    snapshot_key: snap_key,
                    artifact_path: h.artifacts.first().cloned().unwrap_or_default(),
                    handoff_summary_preview: h.summary.chars().take(240).collect(),
                    achieved_at: h.achieved_at,
                });
            }
        }
    }

    rows.sort_by(|a, b| b.snapshot_key.cmp(&a.snapshot_key));
    rows.truncate(lim);
    Ok(rows)
}

/// Return bundled default plan JSON for UI template preview (does not write to disk).
pub async fn graph_preview_template(_ctx: &CommandContext) -> Result<String, String> {
    Ok(novel_graph::default_plan_json().to_string())
}

/// Apply bundled template as formal plan (UI button). Emits graph-state-changed + graph-plan-committed.
pub async fn graph_apply_template(
    ctx: &CommandContext,
    force: Option<bool>,
) -> Result<GraphStateSnapshot, String> {
    let root = work_root(ctx).await;
    let force = force.unwrap_or(false);
    if novel_graph::plan_exists(&root) && !force {
        return Err("plan-graph.json already exists — open Graph or pass force".into());
    }
    if force && novel_graph::plan_exists(&root) {
        let _ = std::fs::remove_file(novel_graph::plan_path(&root));
        let _ = std::fs::remove_file(novel_graph::state_path(&root));
    }
    novel_graph::write_default_plan_file(&root).map_err(|e| e.to_string())?;
    let t = load(&root)?;
    let snap = build_snapshot(&t);
    emit_graph_state(&ctx.app_handle, &snap);
    let _ = ctx.app_handle.emit(
        "graph-plan-committed",
        serde_json::json!({ "source": "ipc" }),
    );
    Ok(snap)
}
