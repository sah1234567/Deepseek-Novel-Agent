//! Persist plan-graph.json / graph-state.json / graph.jsonl / handoffs.

use crate::error::{GraphError, GraphResult};
use crate::types::{
    GraphState, NodeHandoff, PlanGraph, GRAPH_JSONL_REL, GRAPH_STATE_REL, HANDOFFS_DIR_REL,
    PLAN_GRAPH_REL,
};
use serde_json::Value;
use std::fs;
use std::io::Write;
use std::path::Path;

pub fn plan_path(work_root: &Path) -> std::path::PathBuf {
    work_root.join(PLAN_GRAPH_REL)
}

pub fn state_path(work_root: &Path) -> std::path::PathBuf {
    work_root.join(GRAPH_STATE_REL)
}

pub fn load_plan(work_root: &Path) -> GraphResult<Option<PlanGraph>> {
    let p = plan_path(work_root);
    if !p.exists() {
        return Ok(None);
    }
    let s = fs::read_to_string(&p)?;
    // Deserialize only — validation is deferred to commit-time (GraphCommitPlan / PlanBuilder.commit).
    // Empty skeleton is a valid state (no workflow defined yet).
    let plan: PlanGraph = serde_json::from_str(&s).map_err(|e| {
        // Detect legacy format (old NodeKind, LoopCursor, LoopUntil, target_chapters) and give a clear message.
        if s.contains("\"kind\"") || s.contains("\"chapter\":") && s.contains("\"volume\":") {
            GraphError::Validation(
                "plan-graph.json uses deprecated format (NodeKind/LoopCursor/LoopUntil/target_chapters). \
                 Please ask the Agent to migrate it: '请帮我迁移旧版计划到新格式' — the Agent will read \
                 the old plan and rebuild it with PlanBuilder.".into(),
            )
        } else {
            GraphError::Validation(format!("plan-graph.json parse error: {e}"))
        }
    })?;
    Ok(Some(plan))
}

pub fn save_plan(work_root: &Path, plan: &PlanGraph) -> GraphResult<()> {
    let p = plan_path(work_root);
    if let Some(parent) = p.parent() {
        fs::create_dir_all(parent)?;
    }
    let s = serde_json::to_string_pretty(plan)?;
    fs::write(p, s)?;
    Ok(())
}

pub fn load_state(work_root: &Path) -> GraphResult<GraphState> {
    let p = state_path(work_root);
    if !p.exists() {
        return Ok(GraphState::default());
    }
    let s = fs::read_to_string(&p)?;
    Ok(serde_json::from_str(&s)?)
}

pub fn save_state(work_root: &Path, state: &GraphState) -> GraphResult<()> {
    let p = state_path(work_root);
    if let Some(parent) = p.parent() {
        fs::create_dir_all(parent)?;
    }
    let s = serde_json::to_string_pretty(state)?;
    fs::write(p, s)?;
    Ok(())
}

pub fn append_jsonl(work_root: &Path, event: &Value) -> GraphResult<()> {
    let p = work_root.join(GRAPH_JSONL_REL);
    if let Some(parent) = p.parent() {
        fs::create_dir_all(parent)?;
    }
    let mut f = fs::OpenOptions::new().create(true).append(true).open(p)?;
    writeln!(f, "{}", serde_json::to_string(event)?)?;
    Ok(())
}

pub fn save_handoff(work_root: &Path, handoff: &NodeHandoff) -> GraphResult<()> {
    let dir = work_root.join(HANDOFFS_DIR_REL);
    fs::create_dir_all(&dir)?;
    let p = dir.join(format!("{}.json", handoff.node_id));
    fs::write(p, serde_json::to_string_pretty(handoff)?)?;
    Ok(())
}

/// Save a snapshot copy of the handoff so history survives loop advance overwrites.
pub fn save_handoff_snapshot(
    work_root: &Path,
    handoff: &NodeHandoff,
    snapshot_key: &str,
) -> GraphResult<()> {
    let dir = work_root.join(HANDOFFS_DIR_REL);
    fs::create_dir_all(&dir)?;
    let p = dir.join(format!("{}-snap-{}.json", handoff.node_id, snapshot_key));
    fs::write(p, serde_json::to_string_pretty(handoff)?)?;
    Ok(())
}

/// List snapshot handoff files for a node, newest first.
pub fn list_handoff_snapshots(
    work_root: &Path,
    node_id: &str,
    limit: usize,
) -> GraphResult<Vec<(String, NodeHandoff)>> {
    let dir = work_root.join(HANDOFFS_DIR_REL);
    if !dir.is_dir() {
        return Ok(Vec::new());
    }
    let prefix = format!("{node_id}-snap-");
    let suffix = ".json";
    let mut entries = Vec::new();
    for e in std::fs::read_dir(&dir)? {
        let e = e?;
        let name = e.file_name().to_string_lossy().into_owned();
        if !name.starts_with(&prefix) || !name.ends_with(suffix) {
            continue;
        }
        let snap_key = name[prefix.len()..name.len() - suffix.len()].to_string();
        let raw = std::fs::read_to_string(e.path())?;
        let Ok(handoff) = serde_json::from_str::<NodeHandoff>(&raw) else {
            continue;
        };
        entries.push((snap_key, handoff));
    }
    entries.sort_by(|(a, _), (b, _)| b.cmp(a));
    entries.truncate(limit);
    Ok(entries)
}

pub fn plan_exists(work_root: &Path) -> bool {
    plan_path(work_root).exists()
}
