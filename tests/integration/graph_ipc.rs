//! Graph IPC / snapshot shape checks (no Tauri AppHandle).

use novel_graph::{build_snapshot, parse_plan, GraphTracker};

#[test]
fn snapshot_matches_fixture_shape() {
    let plan_json = std::fs::read_to_string(
        std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../fixtures/graph/plan_book_loop.json"),
    )
    .unwrap_or_else(|_| novel_graph::default_plan_json().to_string());
    let plan = parse_plan(&plan_json).expect("plan");
    let t = GraphTracker::new(plan);
    let snap = build_snapshot(&t);
    let v = serde_json::to_value(&snap).expect("json");
    assert!(v.get("planVersion").is_some() || v.get("plan_version").is_some());
    assert!(v.get("nodes").and_then(|n| n.as_array()).is_some());
    assert!(v.get("edges").and_then(|n| n.as_array()).is_some());
    assert!(v.get("loops").and_then(|n| n.as_array()).is_some());
    assert!(v.get("runningNodeIds").is_some() || v.get("running_node_ids").is_some());
    assert!(v.get("enforceGates").is_some() || v.get("enforce_gates").is_some());

    let golden = std::fs::read_to_string(
        std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../fixtures/graph/snapshot_ch37.json"),
    )
    .expect("golden");
    let g: serde_json::Value = serde_json::from_str(&golden).expect("parse golden");
    assert_eq!(g["loops"][0]["loopId"], "book-body");
    assert_eq!(g["loops"][0]["cursor"]["chapter"], 37);
}

#[test]
fn loop_changed_fixture_deserializes() {
    let raw = std::fs::read_to_string(
        std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../fixtures/graph/event_graph_loop_changed.json"),
    )
    .expect("fixture");
    let v: serde_json::Value = serde_json::from_str(&raw).expect("json");
    assert_eq!(v["loopId"], "book-body");
    assert_eq!(v["chapter"], 38);
    assert!(v["resetNodeIds"].as_array().unwrap().len() >= 1);
}

#[test]
fn handoff_fixture_has_required_fields() {
    let raw = std::fs::read_to_string(
        std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../fixtures/graph/handoff_outline.json"),
    )
    .expect("fixture");
    let h: novel_graph::NodeHandoff = serde_json::from_str(&raw).expect("handoff");
    assert!(!h.summary.is_empty());
    assert!(!h.files_touched.is_empty());
    assert!(!h.artifacts.is_empty());
}
