//! Graph tracker / handoff / loop / gates integration tests.

use novel_graph::{
    build_snapshot, check_write_allowed, ensure_graph_initialized, write_default_plan_file,
    parse_plan, parse_regate_directive, save_checkpoint, GraphCheckpoint, GraphTracker, NodeStatus,
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
    t.start_node("world-bible", None).unwrap();
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
    t.start_node("world-bible", None).unwrap();
    let err = t.approve(tmp.path(), "world-bible");
    assert!(err.is_err());
}

#[test]
fn loop_advance_on_sync_canon() {
    let (tmp, mut t) = setup();
    // Force stations ready without full chain for loop test: mark outline deps achieved
    for id in ["world-bible", "outline"] {
        t.state.nodes.get_mut(id).unwrap().status = NodeStatus::Achieved;
        t.state.nodes.get_mut(id).unwrap().pending_summary = Some("ok".into());
        t.state.nodes.get_mut(id).unwrap().handoff = Some(novel_graph::NodeHandoff {
            node_id: id.into(),
            summary: "ok".into(),
            files_touched: vec![],
            artifacts: vec![],
            achieved_at: None,
        });
    }
    t.recompute_ready();
    t.set_loop_target("book-body", 2).unwrap();
    for id in ["ensure-fine-outline", "write-chapter", "sync-canon"] {
        t.start_node(id, None).unwrap();
        t.set_pending_summary(id, format!("done {id}")).unwrap();
        t.record_file_touch(id, "chapters/chapter-001.md", "update")
            .unwrap();
        // skip human
        t.state.nodes.get_mut(id).unwrap().status = NodeStatus::Verifying;
        let adv = t.approve(tmp.path(), id).unwrap();
        if id == "sync-canon" {
            assert!(adv.is_some());
            assert_eq!(adv.unwrap().chapter, 2);
            assert_eq!(t.state.loops.get("book-body").unwrap().cursor.chapter, 2);
        }
    }
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
    assert!(!t.plan.nodes.is_empty());
}

#[test]
fn write_default_plan_noop_when_plan_exists() {
    let (tmp, _) = setup();
    let plan_mtime = fs::metadata(tmp.path().join("knowledge/meta/plan-graph.json"))
        .unwrap()
        .modified()
        .unwrap();
    write_default_plan_file(tmp.path()).unwrap();
    let after = fs::metadata(tmp.path().join("knowledge/meta/plan-graph.json"))
        .unwrap()
        .modified()
        .unwrap();
    assert_eq!(plan_mtime, after);
}

#[test]
fn checkpoint_roundtrip() {
    let tmp = TempDir::new().unwrap();
    let cp = GraphCheckpoint {
        loop_id: "book-body".into(),
        cursor: novel_graph::LoopCursor {
            chapter: 3,
            ..Default::default()
        },
        frozen_at: "2026-01-01T00:00:00Z".into(),
        artifact_paths: vec!["chapters/chapter-003.md".into()],
    };
    save_checkpoint(tmp.path(), &cp).unwrap();
    let loaded = novel_graph::load_checkpoint(tmp.path(), "book-body")
        .unwrap()
        .expect("exists");
    assert_eq!(loaded.cursor.chapter, 3);
    assert_eq!(loaded.artifact_paths.len(), 1);
}

#[test]
fn parse_regate_directive_integration() {
    let got = parse_regate_directive("REGATE: sync-canon\nREASON: canon drift\n").unwrap();
    assert_eq!(got.0, "sync-canon");
    assert_eq!(got.1.as_deref(), Some("canon drift"));
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
