//! Unit tests for GraphTracker reopen / demote / gates / init / handoff listing (CRAP coverage).

#[cfg(test)]
mod tracker_coverage {
    use crate::{
        check_write_allowed, default_plan, ensure_graph_initialized, list_handoff_chapters,
        load_handoff, save_handoff_chapter, save_plan, write_default_plan_file, GraphTracker,
        NodeHandoff, NodeStatus,
    };
    use tempfile::TempDir;

    fn tracker() -> (TempDir, GraphTracker) {
        let tmp = TempDir::new().unwrap();
        let plan = default_plan().unwrap();
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
    fn set_loop_target_requires_known_loop() {
        let (_tmp, mut t) = tracker();
        assert!(t.set_loop_target("missing-loop", 10).is_err());
        t.set_loop_target("book-body", 12).unwrap();
        assert_eq!(t.state.settings.target_chapters, Some(12));
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
        // force running without deps achieved
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
        let mut t = ensure_graph_initialized(tmp.path()).unwrap();
        t.start_node("world-bible", None).unwrap();
        t.set_pending_summary("world-bible", "bible done".into())
            .unwrap();
        t.record_file_touch(
            "world-bible",
            "knowledge/shared-systems/背景设定.md",
            "create",
        )
        .unwrap();
        t.state.nodes.get_mut("world-bible").unwrap().status = NodeStatus::Verifying;
        // bypass human gate for handoff persist
        t.node_plan("world-bible").unwrap();
        {
            // human required — set awaiting then approve needs summary already set
            t.state.nodes.get_mut("world-bible").unwrap().status = NodeStatus::AwaitingApproval;
        }
        t.approve(tmp.path(), "world-bible").unwrap();
        let h = load_handoff(tmp.path(), "world-bible")
            .unwrap()
            .expect("handoff");
        assert!(h.summary.contains("bible"));
    }

    #[test]
    fn ensure_initialized_persists_missing_state() {
        let tmp = TempDir::new().unwrap();
        let plan = default_plan().unwrap();
        save_plan(tmp.path(), &plan).unwrap();
        assert!(!tmp.path().join("knowledge/meta/graph-state.json").exists());
        let _ = ensure_graph_initialized(tmp.path()).unwrap();
        assert!(tmp.path().join("knowledge/meta/graph-state.json").exists());
    }

    #[test]
    fn list_handoff_chapters_empty_when_dir_missing() {
        let tmp = TempDir::new().unwrap();
        let entries = list_handoff_chapters(tmp.path(), "write-chapter", 5).unwrap();
        assert!(entries.is_empty());
    }

    #[test]
    fn list_handoff_chapters_filters_sorts_and_limits() {
        let tmp = TempDir::new().unwrap();
        let handoff = |id: &str, summary: &str| NodeHandoff {
            node_id: id.into(),
            summary: summary.into(),
            files_touched: vec![],
            artifacts: vec![],
            achieved_at: None,
        };
        save_handoff_chapter(tmp.path(), &handoff("write-chapter", "ch1"), 1).unwrap();
        save_handoff_chapter(tmp.path(), &handoff("write-chapter", "ch3"), 3).unwrap();
        save_handoff_chapter(tmp.path(), &handoff("write-chapter", "ch2"), 2).unwrap();
        save_handoff_chapter(tmp.path(), &handoff("other", "x"), 9).unwrap();
        // malformed chapter / json should be skipped
        let dir = tmp.path().join("knowledge/meta/handoffs");
        std::fs::write(dir.join("write-chapter-ch-bad.json"), "{}").unwrap();
        std::fs::write(dir.join("write-chapter-ch-004.json"), "not-json").unwrap();

        let entries = list_handoff_chapters(tmp.path(), "write-chapter", 2).unwrap();
        assert_eq!(entries.len(), 2);
        assert_eq!(entries[0].0, 3);
        assert_eq!(entries[1].0, 2);
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

        t.set_loop_cursor("book-body", 5).unwrap();

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
        assert_eq!(t.state.loops.get("book-body").unwrap().cursor.chapter, 5);
        assert_eq!(t.state.loops.get("book-body").unwrap().cursor.round, 5);
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
        // Simulate sticky flag that would leak across chapters if not cleared on reopen.
        t.state.nodes.get_mut("write-chapter").unwrap().human_intervened = true;
        t.recompute_ready();
        t.start_node("sync-canon", None).unwrap();
        t.set_pending_summary("sync-canon", "synced".into())
            .unwrap();
        assert!(t.approve(tmp.path(), "sync-canon").unwrap().is_some());
        assert!(
            !t.state.nodes.get("write-chapter").unwrap().human_intervened,
            "chapter N intervention must not stick after loop advance"
        );
    }

    #[test]
    fn set_loop_cursor_unknown_loop_errors() {
        let (_tmp, mut t) = tracker();
        assert!(t.set_loop_cursor("missing-loop", 2).is_err());
    }

    #[test]
    fn human_intervened_forces_awaiting_approval_even_if_plan_human_false() {
        let (_tmp, mut t) = tracker();
        // ensure-fine-outline has acceptance.human.required = false in default plan
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
}
