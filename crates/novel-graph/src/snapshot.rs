//! Build GraphStateSnapshot for IPC / UI.

use crate::tracker::GraphTracker;
use crate::types::{
    GraphEdgeView, GraphHitlHint, GraphLoopView, GraphNodeView, GraphStateSnapshot, LoopSummary,
    NodeStatus,
};

pub fn build_snapshot(tracker: &GraphTracker) -> GraphStateSnapshot {
    let mut hitl = Vec::new();
    let nodes: Vec<GraphNodeView> = tracker
        .plan
        .nodes
        .iter()
        .map(|n| {
            let rt = tracker.state.nodes.get(&n.id);
            let status = rt.map(|r| r.status).unwrap_or(NodeStatus::Waiting);
            if status == NodeStatus::AwaitingApproval {
                hitl.push(GraphHitlHint {
                    node_id: n.id.clone(),
                    kind: "node_approval".into(),
                    label: "待节点放行".into(),
                });
            }
            let cursor_badge = rt
                .and_then(|r| r.loop_id.as_ref())
                .and_then(|lid| tracker.state.loops.get(lid))
                .map(|l| format!("Ch.{}", l.cursor.chapter));
            GraphNodeView {
                id: n.id.clone(),
                title: n.title.clone(),
                kind: n.kind,
                status,
                loop_id: rt.and_then(|r| r.loop_id.clone()),
                cursor_badge,
                effective_spec: rt.and_then(|r| r.effective_spec.clone()),
                human_intervened: rt.map(|r| r.human_intervened).unwrap_or(false),
            }
        })
        .collect();

    let mut edges = Vec::new();
    for n in &tracker.plan.nodes {
        for d in &n.deps {
            edges.push(GraphEdgeView {
                source: d.clone(),
                target: n.id.clone(),
                kind: "deps".into(),
            });
        }
    }

    let loops: Vec<GraphLoopView> = tracker
        .plan
        .loops
        .iter()
        .map(|lp| {
            let rt = tracker.state.loops.get(&lp.id);
            GraphLoopView {
                loop_id: lp.id.clone(),
                station_ids: lp.stations.clone(),
                cursor: rt
                    .map(|r| r.cursor.clone())
                    .unwrap_or_else(|| lp.cursor.clone()),
                target_chapters: tracker
                    .state
                    .settings
                    .target_chapters
                    .or(tracker.plan.target_chapters),
                phase: rt.map(|r| r.phase).unwrap_or_default(),
                active_station_id: rt.and_then(|r| r.active_station_id.clone()),
                last_advance_at: rt.and_then(|r| r.last_advance_at.clone()),
                paused_reason: rt.and_then(|r| r.paused_reason.clone()),
                world_state_board: lp.world_state_board.clone(),
                entry: lp.entry.clone(),
                advance_after: lp.advance_after.clone(),
            }
        })
        .collect();

    let loop_summaries: Vec<LoopSummary> = loops
        .iter()
        .map(|l| {
            let label = match l.target_chapters {
                Some(t) => format!("Ch.{}/{}", l.cursor.chapter, t),
                None => format!("Ch.{}", l.cursor.chapter),
            };
            LoopSummary {
                loop_id: l.loop_id.clone(),
                cursor_label: label,
                phase: l.phase,
            }
        })
        .collect();

    GraphStateSnapshot {
        plan_version: tracker.plan.version.clone(),
        nodes,
        edges,
        loops,
        focused_node_id: tracker.state.focused_node_id.clone(),
        running_node_ids: tracker.state.running_node_ids.clone(),
        hitl,
        loop_summaries,
        enforce_gates: tracker.state.settings.enforce_gates,
        has_plan: true,
    }
}

/// Empty snapshot when the work has no formal plan yet.
pub fn empty_snapshot() -> GraphStateSnapshot {
    GraphStateSnapshot {
        plan_version: String::new(),
        nodes: Vec::new(),
        edges: Vec::new(),
        loops: Vec::new(),
        focused_node_id: None,
        running_node_ids: Vec::new(),
        hitl: Vec::new(),
        loop_summaries: Vec::new(),
        enforce_gates: false,
        has_plan: false,
    }
}
