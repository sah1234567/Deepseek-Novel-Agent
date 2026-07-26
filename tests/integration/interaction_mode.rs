#![allow(clippy::unwrap_used)]
#![allow(clippy::expect_used)]

//! Interaction mode (orchestrate | work) end-to-end: tools + prompt + IPC field contract.

use novel_core::{
    resolve_tool_visibility, tool_schemas_for_visibility, DynamicContext, InteractionMode,
    SystemPromptBuilder, ToolVisibility,
};
use novel_graph::{parse_plan, GraphTracker};
use novel_tools::default_registry;
use std::fs;
use tempfile::TempDir;

fn load_fixture_plan(tmp: &TempDir) -> GraphTracker {
    let plan = fs::read_to_string(
        std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../fixtures/graph/plan_book_loop.json"),
    )
    .expect("fixture");
    let plan = parse_plan(&plan).expect("plan");
    novel_graph::save_plan(tmp.path(), &plan).unwrap();
    let t = GraphTracker::new(plan);
    t.save(tmp.path()).unwrap();
    t
}

fn tool_names(visibility: ToolVisibility) -> Vec<String> {
    let reg = default_registry();
    tool_schemas_for_visibility(&reg, visibility)
        .into_iter()
        .map(|(n, _, _)| n)
        .collect()
}

#[test]
fn visibility_matrix_orchestrate_vs_work() {
    let tmp = TempDir::new().unwrap();
    let mut t = load_fixture_plan(&tmp);

    assert_eq!(
        resolve_tool_visibility(tmp.path(), InteractionMode::Orchestrate),
        ToolVisibility::Orchestrator
    );
    let orch = tool_names(ToolVisibility::Orchestrator);
    assert!(orch.contains(&"GraphAdvance".into()));
    assert!(!orch.contains(&"Write".into()));

    t.set_focus(Some("world-bible".into())).unwrap();
    t.save(tmp.path()).unwrap();
    assert_eq!(
        resolve_tool_visibility(tmp.path(), InteractionMode::Orchestrate),
        ToolVisibility::Orchestrator
    );

    assert_eq!(
        resolve_tool_visibility(tmp.path(), InteractionMode::Work),
        ToolVisibility::NodeExecution
    );
    let node = tool_names(ToolVisibility::NodeExecution);
    assert!(node.contains(&"Write".into()));
    assert!(!node.contains(&"PlanBuilder".into()));
    assert!(!node.contains(&"GraphAdvance".into()));

    t.set_focus(None).unwrap();
    t.save(tmp.path()).unwrap();
    assert_eq!(
        resolve_tool_visibility(tmp.path(), InteractionMode::Work),
        ToolVisibility::Orchestrator
    );
}

#[test]
fn prompt_layers_differ_by_interaction_mode() {
    let b = SystemPromptBuilder::new();
    let orch = b.build(
        &DynamicContext::default(),
        false,
        InteractionMode::Orchestrate,
    );
    let work = b.build(&DynamicContext::default(), false, InteractionMode::Work);
    // Use ASCII markers + \u escapes so Windows tooling cannot corrupt literals.
    assert!(orch.contains("\u{56fe}\u{7f16}\u{6392}\u{5668}")); // ????
    assert!(!orch.contains("Work\u{ff09}")); // ???Work?
    assert!(
        work.contains("\u{8282}\u{70b9}\u{6267}\u{884c}") // ????
            || work.contains("Work\u{ff09}")
    );
    assert!(!work.contains("\u{56fe}\u{7f16}\u{6392}\u{5668}"));
}

#[test]
fn ipc_app_status_camel_case_contract() {
    // Mirrors novel_server::tauri::AppStatus serde(rename_all = "camelCase")
    // and ui/src/hooks/useAppStatus.ts AppStatus.interactionMode.
    let payload = serde_json::json!({
        "sessionId": "s1",
        "permissionMode": "normal",
        "interactionMode": "work",
        "focusedNodeId": "world-bible",
        "hookRunning": false,
        "pendingUserQuestion": false,
        "turnInProgress": false,
        "turnNumber": 1,
        "projectInitialized": true,
        "todos": [],
        "sessionCacheHit": 0,
        "sessionCacheMiss": 0,
        "sessionCompletion": 0,
        "contextTokens": 0,
        "activeWorkName": "default",
        "graphHitlCount": 0
    });
    assert_eq!(payload["interactionMode"], "work");
    assert!(payload.get("interaction_mode").is_none());
}

#[test]
fn interaction_mode_parse_aliases_match_ipc() {
    assert_eq!(
        InteractionMode::parse("orchestrate").unwrap(),
        InteractionMode::Orchestrate
    );
    assert_eq!(
        InteractionMode::parse("work").unwrap(),
        InteractionMode::Work
    );
    assert!(InteractionMode::parse("invalid").is_err());
}
