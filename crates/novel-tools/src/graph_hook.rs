//! Write/Edit graph side-effects: optional gate, file journal, demote-on-edit.
//!
//! - [`graph_gate_write`]: when `enforce_gates`, path must be writable by focused node.
//! - [`graph_record_write`]: journal touch; demote Achieved deliverables outside world_state_board;
//!   persist state; notify UI via `on_graph_state_changed` when demotion happened.

use crate::{ToolContext, ToolError};
use novel_graph::{check_write_allowed, GraphTracker};
use serde_json::json;

pub fn graph_gate_write(ctx: &ToolContext, rel_path: &str) -> Result<(), ToolError> {
    let Ok(Some(t)) = GraphTracker::load(&ctx.project_root) else {
        return Ok(());
    };
    if !t.state.settings.enforce_gates {
        return Ok(());
    }
    let writer = t.state.focused_node_id.clone();
    check_write_allowed(&t, rel_path, writer.as_deref())
        .map_err(|e| ToolError::Execution(e.to_string()))
}

pub fn graph_record_write(ctx: &ToolContext, rel_path: &str, op: &str) {
    let Ok(Some(mut t)) = GraphTracker::load(&ctx.project_root) else {
        return;
    };
    let Some(node_id) = t
        .state
        .focused_node_id
        .clone()
        .or_else(|| t.state.running_node_ids.first().cloned())
    else {
        return;
    };
    if let Err(e) = t.record_file_touch(&node_id, rel_path, op) {
        tracing::warn!(error = %e, node_id, rel_path, op, "graph record_file_touch failed");
    }
    if ctx.mark_graph_intervention_on_write {
        if let Err(e) = t.mark_human_intervened(&node_id) {
            tracing::warn!(error = %e, node_id, "graph mark_human_intervened failed");
        }
    }
    // demote Achieved deliverables edited outside intentional sync (world_state_board exempt)
    let demoted = t.demote_on_edit(rel_path);
    if !demoted.is_empty() {
        tracing::info!(?demoted, path = rel_path, "graph demote_on_edit");
        if let Err(e) = novel_graph::append_jsonl(
            &ctx.project_root,
            &json!({
                "event": "demote_on_edit",
                "path": rel_path,
                "op": op,
                "demoted": demoted,
            }),
        ) {
            tracing::warn!(error = %e, "graph demote_on_edit jsonl failed");
        }
    }
    if let Err(e) = t.save(&ctx.project_root) {
        tracing::warn!(error = %e, "graph save after record_write failed");
        return;
    }
    if !demoted.is_empty() || ctx.mark_graph_intervention_on_write {
        if let Some(cb) = &ctx.on_graph_state_changed {
            cb();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ToolContext;
    use novel_graph::{save_plan, GraphTracker, NodeStatus, PlanGraph, PlanNode};
    use std::sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    };
    use tempfile::TempDir;

    /// Build a minimal plan with nodes needed for graph_hook tests.
    fn minimal_plan() -> PlanGraph {
        PlanGraph {
            version: "1".into(),
            max_parallel_nodes: 4,
            nodes: vec![
                PlanNode {
                    id: "world-bible".into(),
                    title: "WB".into(),
                    spec: Some("x".into()),
                    tags: vec!["world_bible".into()],
                    ..Default::default()
                },
                PlanNode {
                    id: "outline".into(),
                    title: "OL".into(),
                    spec: Some("x".into()),
                    deps: vec!["world-bible".into()],
                    tags: vec!["outline".into()],
                    artifacts: vec![novel_graph::ArtifactRef {
                        path: Some("knowledge/plot/大纲.md".into()),
                        role: novel_graph::ArtifactRole::PrimaryDeliverable,
                        path_template: None,
                    }],
                    ..Default::default()
                },
                PlanNode {
                    id: "ensure-fine-outline".into(),
                    title: "EFO".into(),
                    spec: Some("x".into()),
                    deps: vec!["outline".into()],
                    tags: vec!["ensure_fine_outline".into()],
                    ..Default::default()
                },
            ],
            ..Default::default()
        }
    }

    fn seeded_work() -> TempDir {
        let tmp = TempDir::new().unwrap();
        let plan = minimal_plan();
        save_plan(tmp.path(), &plan).unwrap();
        let t = GraphTracker::new(plan);
        t.save(tmp.path()).unwrap();
        tmp
    }

    #[test]
    fn record_write_noop_without_graph() {
        let tmp = TempDir::new().unwrap();
        let ctx = ToolContext::new(tmp.path().to_path_buf());
        graph_record_write(&ctx, "knowledge/plot/大纲.md", "update");
    }

    #[test]
    fn record_write_noop_without_focused_or_running() {
        let tmp = seeded_work();
        let ctx = ToolContext::new(tmp.path().to_path_buf());
        graph_record_write(&ctx, "knowledge/plot/大纲.md", "update");
    }

    #[test]
    fn record_write_journals_and_demotes_with_callback() {
        let tmp = seeded_work();
        {
            let mut t = GraphTracker::load(tmp.path()).unwrap().unwrap();
            t.state.nodes.get_mut("world-bible").unwrap().status = NodeStatus::Achieved;
            t.state.nodes.get_mut("outline").unwrap().status = NodeStatus::Achieved;
            t.state.focused_node_id = Some("ensure-fine-outline".into());
            t.save(tmp.path()).unwrap();
        }
        let fired = Arc::new(AtomicBool::new(false));
        let flag = Arc::clone(&fired);
        let mut ctx = ToolContext::new(tmp.path().to_path_buf());
        ctx.on_graph_state_changed = Some(Arc::new(move || {
            flag.store(true, Ordering::SeqCst);
        }));

        graph_record_write(&ctx, "knowledge/plot/大纲.md", "update");

        assert!(fired.load(Ordering::SeqCst));
        let t = GraphTracker::load(tmp.path()).unwrap().unwrap();
        // outline should be demoted because its artifact was edited outside world_state_board
        assert!(!matches!(
            t.state.nodes.get("outline").unwrap().status,
            NodeStatus::Achieved
        ));
    }

    #[test]
    fn gate_write_ok_when_gates_disabled() {
        let tmp = seeded_work();
        {
            let mut t = GraphTracker::load(tmp.path()).unwrap().unwrap();
            t.state.settings.enforce_gates = false;
            t.save(tmp.path()).unwrap();
        }
        let ctx = ToolContext::new(tmp.path().to_path_buf());
        assert!(graph_gate_write(&ctx, "chapters/chapter-001.md").is_ok());
    }
}
