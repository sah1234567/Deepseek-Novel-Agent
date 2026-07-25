//! Shared Tauri emits for graph UI (IPC mutate path + tool Event forwarder).

use novel_graph::GraphStateSnapshot;
use tauri::{AppHandle, Emitter};

/// `graph-state-changed` plus HITL hints when present.
pub fn emit_graph_state(app: &AppHandle, snapshot: &GraphStateSnapshot) {
    if let Err(e) = app.emit("graph-state-changed", snapshot) {
        tracing::warn!(error = %e, "emit graph-state-changed failed");
    }
    emit_graph_hitl(app, snapshot);
}

pub fn emit_graph_hitl(app: &AppHandle, snapshot: &GraphStateSnapshot) {
    if snapshot.hitl.is_empty() {
        return;
    }
    if let Err(e) = app.emit("graph-hitl", &snapshot.hitl) {
        tracing::warn!(error = %e, "emit graph-hitl failed");
    }
    for h in &snapshot.hitl {
        if h.kind == "node_approval" {
            let _ = app.emit(
                "graph-approval-required",
                serde_json::json!({ "nodeId": h.node_id, "label": h.label }),
            );
        }
    }
}

/// Book Loop advance — same shape as IPC `graph_approve` when a loop rolls.
pub fn emit_graph_loop_advanced(
    app: &AppHandle,
    loop_id: &str,
    chapter: u32,
    reset_node_ids: &[String],
    snapshot: &GraphStateSnapshot,
) {
    let payload = serde_json::json!({
        "loopId": loop_id,
        "chapter": chapter,
        "resetNodeIds": reset_node_ids,
        "loops": snapshot.loops,
    });
    if let Err(e) = app.emit("graph-loop-changed", &payload) {
        tracing::warn!(error = %e, "emit graph-loop-changed failed");
    }
    for id in reset_node_ids {
        let _ = app.emit(
            "node-session-reset",
            serde_json::json!({
                "nodeId": id,
                "reason": "loop_advance",
                "chapter": chapter,
            }),
        );
    }
}

pub fn emit_graph_loops_only(app: &AppHandle, snapshot: &GraphStateSnapshot) {
    if snapshot.loops.is_empty() {
        return;
    }
    let _ = app.emit(
        "graph-loop-changed",
        serde_json::json!({ "loops": snapshot.loops }),
    );
}

/// After tool-only events (no snapshot payload): reload disk and fan out HITL / loops.
pub fn emit_graph_side_effects_from_disk(app: &AppHandle, work_root: &std::path::Path) {
    let Ok(Some(t)) = novel_graph::GraphTracker::load(work_root) else {
        return;
    };
    let snapshot = novel_graph::build_snapshot(&t);
    emit_graph_hitl(app, &snapshot);
    emit_graph_loops_only(app, &snapshot);
}
