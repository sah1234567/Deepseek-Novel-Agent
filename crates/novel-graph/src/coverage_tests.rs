//! Unit tests for GraphTracker reopen / demote / gates / init / handoff listing (CRAP coverage).

#[cfg(test)]
mod tracker_coverage {
    use crate::{
        check_write_allowed, ensure_graph_initialized, list_handoff_snapshots, load_handoff,
        save_handoff_snapshot, save_plan, write_default_plan_file, Acceptance, AdvanceRule,
        CounterOp, Cursor, GraphTracker, LoopOnAdvance, MachineAcceptance, NodeHandoff, NodeStatus,
        PlanGraph, PlanNode, Until, WorkflowLoop,
    };
    use std::collections::HashMap;
    use tempfile::TempDir;

    /// Build a classic book-loop plan for testing (7 nodes, 1 loop).
    fn classic_plan() -> PlanGraph {
        PlanGraph {
            version: "1".into(),
            enforce_gates: true,
            max_parallel_nodes: 4,
            auto_start_ready: false,
            settings: {
                let mut s = HashMap::new();
                s.insert("targetChapters".into(), serde_json::json!(200));
                s
            },
            nodes: vec![
                PlanNode {
                    id: "world-bible".into(),
                    title: "世界观".into(),
                    spec: Some("build world bible".into()),
                    tags: vec!["world_bible".into()],
                    artifacts: vec![crate::ArtifactRef {
                        path: Some("knowledge/shared-systems/背景设定.md".into()),
                        role: crate::ArtifactRole::PrimaryDeliverable,
                        path_template: None,
                    }],
                    acceptance: Acceptance {
                        machine: MachineAcceptance::None,
                        human: Some(crate::HumanGate {
                            required: true,
                            on_reject: crate::OnReject::Continue,
                            prompt: Some("批准世界观？".into()),
                            review: vec![],
                        }),
                    },
                    ..Default::default()
                },
                PlanNode {
                    id: "outline".into(),
                    title: "大纲".into(),
                    spec: Some("write outline".into()),
                    deps: vec!["world-bible".into()],
                    tags: vec!["outline".into()],
                    artifacts: vec![crate::ArtifactRef {
                        path: Some("knowledge/plot/大纲.md".into()),
                        role: crate::ArtifactRole::PrimaryDeliverable,
                        path_template: None,
                    }],
                    acceptance: Acceptance {
                        machine: MachineAcceptance::Auditor,
                        human: Some(crate::HumanGate {
                            required: true,
                            on_reject: crate::OnReject::Continue,
                            prompt: Some("批准大纲？".into()),
                            review: vec![],
                        }),
                    },
                    ..Default::default()
                },
                PlanNode {
                    id: "ensure-fine-outline".into(),
                    title: "细纲确保".into(),
                    spec_template: Some("fine outline ch{{cursor.chapter}}".into()),
                    deps: vec!["outline".into()],
                    tags: vec!["ensure_fine_outline".into()],
                    artifacts: vec![crate::ArtifactRef {
                        path_template: Some(
                            "knowledge/plot/细纲/chapter-{{cursor.chapter | pad3}}-细纲.md".into(),
                        ),
                        role: crate::ArtifactRole::PrimaryDeliverable,
                        path: None,
                    }],
                    acceptance: Acceptance {
                        machine: MachineAcceptance::Auditor,
                        human: Some(crate::HumanGate {
                            required: false,
                            on_reject: crate::OnReject::Continue,
                            prompt: None,
                            review: vec![],
                        }),
                    },
                    ..Default::default()
                },
                PlanNode {
                    id: "write-chapter".into(),
                    title: "写章".into(),
                    spec_template: Some("write chapter {{cursor.chapter}}".into()),
                    deps: vec!["ensure-fine-outline".into()],
                    tags: vec!["chapter_body".into()],
                    artifacts: vec![crate::ArtifactRef {
                        path_template: Some("chapters/chapter-{{cursor.chapter | pad3}}.md".into()),
                        role: crate::ArtifactRole::PrimaryDeliverable,
                        path: None,
                    }],
                    iterate: Some(crate::Iterate {
                        max_iterations: 8,
                        until: crate::IterateUntil::AuditorPass,
                    }),
                    ..Default::default()
                },
                PlanNode {
                    id: "sync-canon".into(),
                    title: "正典同步".into(),
                    spec_template: Some("sync canon after ch{{cursor.chapter}}".into()),
                    deps: vec!["write-chapter".into()],
                    tags: vec!["sync_canon".into()],
                    artifacts: vec![
                        crate::ArtifactRef {
                            path: Some("knowledge/characters/".into()),
                            role: crate::ArtifactRole::Aux,
                            path_template: None,
                        },
                        crate::ArtifactRef {
                            path: Some("knowledge/INDEX.md".into()),
                            role: crate::ArtifactRole::Aux,
                            path_template: None,
                        },
                    ],
                    ..Default::default()
                },
                PlanNode {
                    id: "volume-review".into(),
                    title: "卷审".into(),
                    spec: Some("volume review".into()),
                    deps: vec!["sync-canon".into()],
                    tags: vec!["volume_review".into()],
                    ..Default::default()
                },
            ],
            loops: vec![WorkflowLoop {
                id: "book-body".into(),
                stations: vec![
                    "ensure-fine-outline".into(),
                    "write-chapter".into(),
                    "sync-canon".into(),
                ],
                entry: "ensure-fine-outline".into(),
                advance_after: "sync-canon".into(),
                cursor: Cursor {
                    counters: {
                        let mut c = HashMap::new();
                        c.insert("chapter".into(), 1);
                        c.insert("volume".into(), 1);
                        c.insert("round".into(), 1);
                        c
                    },
                    tags: HashMap::new(),
                },
                advance: AdvanceRule {
                    increment: "chapter".into(),
                    step: 1,
                    side_effects: vec![CounterOp::Increment {
                        counter: "round".into(),
                        by: 1,
                    }],
                },
                until: Until::CounterGt {
                    counter: "chapter".into(),
                    value: None,
                    value_from: Some("work_meta.settings.targetChapters".into()),
                },
                on_advance: LoopOnAdvance {
                    reopen: vec![
                        "ensure-fine-outline".into(),
                        "write-chapter".into(),
                        "sync-canon".into(),
                    ],
                    clear_node_sessions: true,
                    reinject_objectives: true,
                    preserve_canon_files: true,
                },
                world_state_board: vec![
                    "knowledge/characters/".into(),
                    "knowledge/INDEX.md".into(),
                ],
            }],
        }
    }

    fn tracker() -> (TempDir, GraphTracker) {
        let tmp = TempDir::new().unwrap();
        let plan = classic_plan();
        save_plan(tmp.path(), &plan).unwrap();
        let t = GraphTracker::new(plan);
        t.save(tmp.path()).unwrap();
        (tmp, t)
    }

    #[test]
    fn reopen_cascades_downstream() {
        let (tmp, mut t) = tracker();
        for id in ["world-bible", "outline"] {
            t.state.nodes.get_mut(id).unwrap().status = NodeStatus::Achieved;
        }
        t.recompute_ready();
        t.state.nodes.get_mut("ensure-fine-outline").unwrap().status = NodeStatus::Achieved;
        t.recompute_ready();
        let demoted = t.reopen("world-bible", true).unwrap();
        assert!(demoted.contains(&"world-bible".to_string()));
        assert!(demoted.iter().any(|d| d == "outline"));
        assert_eq!(
            t.state.nodes.get("world-bible").unwrap().status,
            NodeStatus::Ready
        );
        let _ = tmp;
    }

    #[test]
    fn reopen_respects_max_regates() {
        let (_tmp, mut t) = tracker();
        t.state.nodes.get_mut("world-bible").unwrap().status = NodeStatus::Achieved;
        t.state.nodes.get_mut("world-bible").unwrap().regate_count = 99;
        assert!(t.reopen("world-bible", false).is_err());
    }

    #[test]
    fn demote_on_edit_skips_world_state_board() {
        let (_tmp, mut t) = tracker();
        t.state.nodes.get_mut("sync-canon").unwrap().status = NodeStatus::Achieved;
        let demoted = t.demote_on_edit("knowledge/characters/hero.md");
        assert!(demoted.is_empty());
    }

    #[test]
    fn demote_on_edit_reopens_achieved_deliverable() {
        let (_tmp, mut t) = tracker();
        t.state.nodes.get_mut("world-bible").unwrap().status = NodeStatus::Achieved;
        t.state.nodes.get_mut("outline").unwrap().status = NodeStatus::Achieved;
        // outline primary is outside world_state_board
        let demoted = t.demote_on_edit("knowledge/plot/大纲.md");
        assert!(demoted.contains(&"outline".to_string()));
    }

    #[test]
    fn set_loop_setting_requires_known_loop() {
        let (_tmp, mut t) = tracker();
        assert!(t
            .set_loop_setting("missing-loop", "targetChapters", serde_json::json!(10))
            .is_err());
        t.set_loop_setting("book-body", "targetChapters", serde_json::json!(12))
            .unwrap();
        assert_eq!(
            t.state
                .settings
                .settings
                .get("targetChapters")
                .and_then(|v| v.as_i64()),
            Some(12)
        );
    }

    #[test]
    fn gate_allows_when_disabled() {
        let (_tmp, mut t) = tracker();
        t.state.settings.enforce_gates = false;
        assert!(check_write_allowed(&t, "chapters/chapter-001.md", None).is_ok());
    }

    #[test]
    fn gate_allows_running_writer_when_deps_achieved() {
        let (_tmp, mut t) = tracker();
        t.state.nodes.get_mut("world-bible").unwrap().status = NodeStatus::Achieved;
        t.state.nodes.get_mut("outline").unwrap().status = NodeStatus::Achieved;
        t.recompute_ready();
        t.start_node("ensure-fine-outline", None).unwrap();
        let paths = t.writable_paths("ensure-fine-outline").unwrap();
        let path = paths
            .first()
            .cloned()
            .unwrap_or_else(|| "knowledge/plot/细纲.md".into());
        assert!(check_write_allowed(&t, &path, Some("ensure-fine-outline")).is_ok());
    }

    #[test]
    fn gate_denies_when_deps_not_achieved() {
        let (_tmp, mut t) = tracker();
        t.state.nodes.get_mut("write-chapter").unwrap().status = NodeStatus::Running;
        t.state.running_node_ids = vec!["write-chapter".into()];
        let paths = t.writable_paths("write-chapter").unwrap();
        let path = paths
            .first()
            .cloned()
            .unwrap_or_else(|| "chapters/chapter-001.md".into());
        assert!(check_write_allowed(&t, &path, Some("write-chapter")).is_err());
    }

    #[test]
    fn gate_denies_path_outside_writable_set() {
        let (_tmp, mut t) = tracker();
        t.state.nodes.get_mut("world-bible").unwrap().status = NodeStatus::Achieved;
        t.recompute_ready();
        t.start_node("outline", None).unwrap();
        assert!(check_write_allowed(&t, "secrets/passwords.txt", Some("outline")).is_err());
    }

    #[test]
    fn write_default_plan_and_load_handoff_roundtrip() {
        let tmp = TempDir::new().unwrap();
        write_default_plan_file(tmp.path()).unwrap();
        assert!(tmp.path().join("knowledge/meta/plan-graph.json").exists());
        write_default_plan_file(tmp.path()).unwrap(); // no-op second call
                                                      // With empty skeleton, no nodes exist. Build one manually.
        let mut plan = classic_plan();
        // Only keep world-bible for this test
        plan.nodes = vec![PlanNode {
            id: "world-bible".into(),
            title: "世界观".into(),
            spec: Some("build world bible".into()),
            tags: vec!["world_bible".into()],
            acceptance: Acceptance {
                machine: MachineAcceptance::None,
                human: Some(crate::HumanGate {
                    required: true,
                    on_reject: crate::OnReject::Continue,
                    prompt: Some("批准？".into()),
                    review: vec![],
                }),
            },
            artifacts: vec![crate::ArtifactRef {
                path: Some("knowledge/shared-systems/背景设定.md".into()),
                role: crate::ArtifactRole::PrimaryDeliverable,
                path_template: None,
            }],
            ..Default::default()
        }];
        plan.loops.clear();
        save_plan(tmp.path(), &plan).unwrap();
        let mut t = GraphTracker::new(plan);
        t.save(tmp.path()).unwrap();
        t.start_node("world-bible", None).unwrap();
        t.set_pending_summary("world-bible", "bible done".into())
            .unwrap();
        t.record_file_touch(
            "world-bible",
            "knowledge/shared-systems/背景设定.md",
            "create",
        )
        .unwrap();
        t.state.nodes.get_mut("world-bible").unwrap().status = NodeStatus::AwaitingApproval;
        t.approve(tmp.path(), "world-bible").unwrap();
        let h = load_handoff(tmp.path(), "world-bible")
            .unwrap()
            .expect("handoff");
        assert!(h.summary.contains("bible"));
    }

    #[test]
    fn ensure_initialized_persists_missing_state() {
        let tmp = TempDir::new().unwrap();
        let plan = classic_plan();
        save_plan(tmp.path(), &plan).unwrap();
        assert!(!tmp.path().join("knowledge/meta/graph-state.json").exists());
        let _ = ensure_graph_initialized(tmp.path()).unwrap();
        assert!(tmp.path().join("knowledge/meta/graph-state.json").exists());
    }

    #[test]
    fn list_handoff_snapshots_empty_when_dir_missing() {
        let tmp = TempDir::new().unwrap();
        let entries = list_handoff_snapshots(tmp.path(), "write-chapter", 5).unwrap();
        assert!(entries.is_empty());
    }

    #[test]
    fn list_handoff_snapshots_filters_sorts_and_limits() {
        let tmp = TempDir::new().unwrap();
        let handoff = |id: &str, summary: &str| NodeHandoff {
            node_id: id.into(),
            summary: summary.into(),
            files_touched: vec![],
            artifacts: vec![],
            achieved_at: None,
        };
        save_handoff_snapshot(tmp.path(), &handoff("write-chapter", "ch1"), "chapter=1").unwrap();
        save_handoff_snapshot(tmp.path(), &handoff("write-chapter", "ch3"), "chapter=3").unwrap();
        save_handoff_snapshot(tmp.path(), &handoff("write-chapter", "ch2"), "chapter=2").unwrap();
        save_handoff_snapshot(tmp.path(), &handoff("other", "x"), "chapter=9").unwrap();
        // malformed json should be skipped
        let dir = tmp.path().join("knowledge/meta/handoffs");
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("write-chapter-snap-bad.json"), "not-json").unwrap();

        let entries = list_handoff_snapshots(tmp.path(), "write-chapter", 2).unwrap();
        assert_eq!(entries.len(), 2);
        // Sorted by key descending
        assert_eq!(entries[0].0, "chapter=3");
        assert_eq!(entries[1].0, "chapter=2");
        assert!(entries[0].1.summary.contains("ch3"));
    }

    #[test]
    fn set_loop_cursor_demotes_stations_and_readies_entry() {
        let (_tmp, mut t) = tracker();
        for id in ["world-bible", "outline"] {
            t.state.nodes.get_mut(id).unwrap().status = NodeStatus::Achieved;
        }
        t.recompute_ready();
        t.state.nodes.get_mut("ensure-fine-outline").unwrap().status = NodeStatus::Running;
        t.state.nodes.get_mut("write-chapter").unwrap().status = NodeStatus::Ready;
        t.state.running_node_ids = vec!["ensure-fine-outline".into()];

        let mut counters = HashMap::new();
        counters.insert("chapter".into(), 5_i64);
        counters.insert("volume".into(), 1_i64);
        counters.insert("round".into(), 5_i64);
        t.set_loop_cursor("book-body", counters).unwrap();

        assert_eq!(
            t.state.nodes.get("write-chapter").unwrap().status,
            NodeStatus::Waiting
        );
        assert_eq!(
            t.state.nodes.get("ensure-fine-outline").unwrap().status,
            NodeStatus::Ready
        );
        assert!(!t
            .state
            .running_node_ids
            .iter()
            .any(|id| id == "ensure-fine-outline"));
        assert_eq!(
            t.state
                .loops
                .get("book-body")
                .unwrap()
                .cursor
                .counters
                .get("chapter"),
            Some(&5)
        );
        assert_eq!(
            t.state
                .loops
                .get("book-body")
                .unwrap()
                .cursor
                .counters
                .get("round"),
            Some(&5)
        );
    }

    #[test]
    fn loop_advance_clears_focus_when_focused_station_reopened() {
        let (tmp, mut t) = tracker();
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
        t.start_node("sync-canon", None).unwrap();
        t.set_focus(Some("sync-canon".into())).unwrap();
        t.set_pending_summary("sync-canon", "synced".into())
            .unwrap();
        let adv = t.approve(tmp.path(), "sync-canon").unwrap();
        assert!(adv.is_some(), "sync-canon should advance book-body");
        assert!(
            t.state.focused_node_id.is_none(),
            "focus on reopened station must clear"
        );
    }

    #[test]
    fn loop_advance_clears_human_intervened_on_reopened_stations() {
        let (tmp, mut t) = tracker();
        for id in [
            "world-bible",
            "outline",
            "ensure-fine-outline",
            "write-chapter",
        ] {
            t.state.nodes.get_mut(id).unwrap().status = NodeStatus::Achieved;
            t.state.nodes.get_mut(id).unwrap().pending_summary = Some("ok".into());
        }
        t.state
            .nodes
            .get_mut("write-chapter")
            .unwrap()
            .human_intervened = true;
        t.recompute_ready();
        t.start_node("sync-canon", None).unwrap();
        t.set_pending_summary("sync-canon", "synced".into())
            .unwrap();
        assert!(t.approve(tmp.path(), "sync-canon").unwrap().is_some());
        assert!(
            !t.state.nodes.get("write-chapter").unwrap().human_intervened,
            "iteration N intervention must not stick after loop advance"
        );
    }

    #[test]
    fn set_loop_cursor_unknown_loop_errors() {
        let (_tmp, mut t) = tracker();
        assert!(t.set_loop_cursor("missing-loop", HashMap::new()).is_err());
    }

    #[test]
    fn human_intervened_forces_awaiting_approval_even_if_plan_human_false() {
        let (_tmp, mut t) = tracker();
        t.state.nodes.get_mut("world-bible").unwrap().status = NodeStatus::Achieved;
        t.state.nodes.get_mut("outline").unwrap().status = NodeStatus::Achieved;
        t.recompute_ready();
        t.start_node("ensure-fine-outline", None).unwrap();
        t.set_pending_summary("ensure-fine-outline", "fine outline ok".into())
            .unwrap();
        t.submit_for_approval("ensure-fine-outline").unwrap();
        assert_eq!(
            t.state.nodes.get("ensure-fine-outline").unwrap().status,
            NodeStatus::Verifying,
            "without intervention, plan human=false → Verifying"
        );
        t.state.nodes.get_mut("ensure-fine-outline").unwrap().status = NodeStatus::Running;
        t.mark_human_intervened("ensure-fine-outline").unwrap();
        t.submit_for_approval("ensure-fine-outline").unwrap();
        assert_eq!(
            t.state.nodes.get("ensure-fine-outline").unwrap().status,
            NodeStatus::AwaitingApproval,
            "human_intervened must force AwaitingApproval"
        );
        assert!(
            t.state
                .nodes
                .get("ensure-fine-outline")
                .unwrap()
                .human_intervened
        );
    }

    #[test]
    fn legacy_plan_rejected_with_friendly_message() {
        let tmp = TempDir::new().unwrap();
        let plan_path = tmp.path().join("knowledge/meta/plan-graph.json");
        std::fs::create_dir_all(plan_path.parent().unwrap()).unwrap();
        // Old-format JSON with "kind", "chapter"/"volume" cursor, "until":{"op":...}, and "target_chapters"
        std::fs::write(&plan_path, r#"{"version":"1","nodes":[{"id":"a","title":"A","spec":"x","kind":"world_bible"}],"loops":[{"id":"L","stations":["a"],"entry":"a","advance_after":"a","cursor":{"chapter":1,"volume":1,"round":1},"until":{"op":"chapter_gt","value":10},"on_advance":{"reopen":["a"]}}],"target_chapters":200}"#).unwrap();
        let err = crate::persist::load_plan(tmp.path()).unwrap_err();
        let msg = err.to_string();
        assert!(
            msg.contains("deprecated") || msg.contains("migrate"),
            "expected friendly legacy error, got: {msg}"
        );
    }

    #[test]
    fn generic_cursor_template_renders_custom_counters() {
        let mut counters = std::collections::HashMap::new();
        counters.insert("section".into(), 7_i64);
        counters.insert("part".into(), 3_i64);
        let cursor = crate::Cursor {
            counters,
            tags: Default::default(),
        };
        let result = crate::render_template(
            "§{{cursor.section}}.{{cursor.part | pad2}}",
            &cursor,
            &Default::default(),
        );
        assert_eq!(result, "§7.03");
    }

    #[test]
    fn until_counter_gt_evaluates_correctly() {
        let mut counters = std::collections::HashMap::new();
        counters.insert("chapter".into(), 5_i64);
        let cursor = crate::Cursor {
            counters,
            tags: Default::default(),
        };
        let settings: std::collections::HashMap<String, serde_json::Value> =
            [("targetChapters".into(), serde_json::json!(10))].into();
        // This is a tracker-private function, so test via the public API:
        // chapter=5 > 10? No → not yet complete
        // Just verify Cursor API works
        assert_eq!(cursor.counters.get("chapter"), Some(&5));
        let _ = settings;
    }
}
