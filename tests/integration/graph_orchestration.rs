#![allow(clippy::unwrap_used)]
#![allow(clippy::expect_used)]

//! Graph tracker / handoff / loop / gates integration tests.

use novel_graph::{
    build_snapshot, check_write_allowed, ensure_graph_initialized, parse_plan,
    parse_regate_directive, GraphTracker, NodeStatus,
};
use std::fs;
use tempfile::TempDir;

fn setup() -> (TempDir, GraphTracker) {
    let tmp = TempDir::new().unwrap();
    let plan = fs::read_to_string(
        std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../fixtures/graph/plan_book_loop.json"),
    )
    .unwrap_or_else(|_| novel_graph::default_plan_json().to_string());
    let plan = parse_plan(&plan).expect("plan");
    novel_graph::save_plan(tmp.path(), &plan).unwrap();
    let t = GraphTracker::new(plan);
    t.save(tmp.path()).unwrap();
    (tmp, t)
}

#[test]
fn ready_after_deps_achieved() {
    let (tmp, mut t) = setup();
    assert_eq!(
        t.state.nodes.get("world-bible").unwrap().status,
        NodeStatus::Ready
    );
    t.start_node("world-bible").unwrap();
    t.set_pending_summary("world-bible", "Created bible and hero card.".into())
        .unwrap();
    t.record_file_touch(
        "world-bible",
        "knowledge/shared-systems/背景设定.md",
        "create",
    )
    .unwrap();
    // human required → awaiting
    t.submit_for_approval("world-bible").unwrap();
    assert_eq!(
        t.state.nodes.get("world-bible").unwrap().status,
        NodeStatus::AwaitingApproval
    );
    t.approve(tmp.path(), "world-bible").unwrap();
    assert_eq!(
        t.state.nodes.get("world-bible").unwrap().status,
        NodeStatus::Achieved
    );
    assert_eq!(
        t.state.nodes.get("outline").unwrap().status,
        NodeStatus::Ready
    );
}

#[test]
fn handoff_requires_summary() {
    let (tmp, mut t) = setup();
    t.start_node("world-bible").unwrap();
    let err = t.approve(tmp.path(), "world-bible");
    assert!(err.is_err());
}

#[test]
fn loop_advance_on_sync_canon() {
    let (tmp, mut t) = setup();
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
    t.start_node("sync-canon").unwrap();
    t.set_pending_summary("sync-canon", "synced".into())
        .unwrap();
    t.record_file_touch("sync-canon", "knowledge/INDEX.md", "update")
        .unwrap();
    let adv = t
        .approve(tmp.path(), "sync-canon")
        .unwrap()
        .expect("sync-canon should advance book-body");
    assert_eq!(adv.counters.get("chapter"), Some(&2));
    assert_eq!(
        t.state
            .loops
            .get("book-body")
            .unwrap()
            .cursor
            .counters
            .get("chapter"),
        Some(&2)
    );
}

#[test]
fn gate_denies_without_running() {
    let (_tmp, t) = setup();
    // enforce_gates true in default plan
    let err = check_write_allowed(&t, "chapters/chapter-001.md", None);
    assert!(err.is_err());
}

#[test]
fn snapshot_contains_loops() {
    let (_tmp, t) = setup();
    let snap = build_snapshot(&t);
    assert!(!snap.loops.is_empty());
    assert_eq!(snap.loops[0].loop_id, "book-body");
    let json = serde_json::to_string_pretty(&snap).unwrap();
    assert!(
        json.contains("stationIds") || json.contains("station_ids") || json.contains("book-body")
    );
}

#[test]
fn ensure_initialized_writes_files() {
    let tmp = TempDir::new().unwrap();
    fs::create_dir_all(tmp.path().join("knowledge/meta")).unwrap();
    let t = ensure_graph_initialized(tmp.path()).unwrap();
    assert!(tmp.path().join("knowledge/meta/plan-graph.json").exists());
    assert!(tmp.path().join("knowledge/meta/graph-state.json").exists());
    // Default scaffold is an empty skeleton (LLM builds via PlanBuilder).
    assert!(t.plan.nodes.is_empty());
    assert!(t.plan.loops.is_empty());
}

#[test]
fn parse_regate_directive_integration() {
    let got = parse_regate_directive("REGATE: sync-canon\nREASON: canon drift\n").unwrap();
    assert_eq!(got, "sync-canon");
}

#[test]
fn reject_respects_max_iterations() {
    let (_tmp, mut t) = setup();
    {
        let rt = t.state.nodes.get_mut("write-chapter").unwrap();
        rt.status = NodeStatus::AwaitingApproval;
        rt.iteration = 8;
        rt.pending_summary = Some("draft".into());
    }
    t.reject("write-chapter", "needs more work").unwrap();
    let rt = t.state.nodes.get("write-chapter").unwrap();
    assert_eq!(rt.status, NodeStatus::AwaitingApproval);
    assert!(rt
        .feedback
        .as_deref()
        .unwrap_or("")
        .contains("max_iterations=8"));
    t.state.nodes.get_mut("write-chapter").unwrap().iteration = 7;
    t.reject("write-chapter", "try again").unwrap();
    assert_eq!(
        t.state.nodes.get("write-chapter").unwrap().status,
        NodeStatus::Running
    );
}
