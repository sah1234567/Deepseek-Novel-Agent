//! Write/Edit graph gates.

use crate::error::{GraphError, GraphResult};
use crate::tracker::GraphTracker;
use crate::types::NodeStatus;

/// Returns Ok(()) if write is allowed for `path` under current running/focused nodes.
pub fn check_write_allowed(
    tracker: &GraphTracker,
    path: &str,
    writer_node_id: Option<&str>,
) -> GraphResult<()> {
    if !tracker.state.settings.enforce_gates {
        return Ok(());
    }
    let path = path.replace('\\', "/");
    let candidates: Vec<String> = if let Some(id) = writer_node_id {
        vec![id.to_string()]
    } else {
        tracker.state.running_node_ids.clone()
    };
    if candidates.is_empty() {
        return Err(GraphError::GateDenied(
            "no running graph node; activate a Ready node before writing".into(),
        ));
    }
    for id in &candidates {
        let rt = tracker
            .state
            .nodes
            .get(id)
            .ok_or_else(|| GraphError::NodeNotFound(id.clone()))?;
        if !matches!(
            rt.status,
            NodeStatus::Running | NodeStatus::Verifying | NodeStatus::AwaitingApproval
        ) {
            continue;
        }
        let paths = tracker.writable_paths(id)?;
        if paths.iter().any(|p| {
            let p = p.replace('\\', "/");
            path == p
                || path.starts_with(&format!("{p}/"))
                || p.ends_with('/') && path.starts_with(&p)
        }) {
            // deps must be Achieved
            let plan_n = tracker.node_plan(id)?;
            for d in &plan_n.deps {
                let st = tracker
                    .state
                    .nodes
                    .get(d)
                    .map(|r| r.status)
                    .unwrap_or(NodeStatus::Waiting);
                if st != NodeStatus::Achieved {
                    return Err(GraphError::GateDenied(format!(
                        "dependency `{d}` not Achieved before writing `{path}`"
                    )));
                }
            }
            return Ok(());
        }
    }
    Err(GraphError::GateDenied(format!(
        "path `{path}` is not in any running node's writable artifacts"
    )))
}
