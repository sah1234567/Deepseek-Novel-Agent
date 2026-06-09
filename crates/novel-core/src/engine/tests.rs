use super::types::{AgentEngine, EngineConfig};
use crate::{AgentError, AgentType, Event, ForkError, Op, TerminalReason};
use rstest::rstest;
use std::sync::atomic::Ordering;
use tempfile::TempDir;
use tokio::sync::mpsc;

fn test_config(tmp: &TempDir) -> EngineConfig {
    EngineConfig {
        project_root: tmp.path().to_path_buf(),
        settings_path: tmp.path().join("settings.json"),
        db_path: tmp.path().join("state.db"),
        skills_dir: tmp.path().join("skills"),
        global_config_path: tmp.path().join(".novel-agent/api_config.json"),
    }
}

#[rstest]
#[tokio::test]
async fn empty_message_returns_validation_error() {
    let tmp = TempDir::new().unwrap();
    std::fs::create_dir_all(tmp.path().join("skills")).unwrap();
    let mut engine = AgentEngine::new(test_config(&tmp)).unwrap();
    let err = engine.handle_message("").await.unwrap_err();
    assert!(matches!(err, AgentError::Validation(_)));
}

#[rstest]
#[tokio::test]
async fn nested_fork_prohibited_when_sub_agent_running() {
    let tmp = TempDir::new().unwrap();
    std::fs::create_dir_all(tmp.path().join("skills")).unwrap();
    let engine = AgentEngine::new(test_config(&tmp)).unwrap();
    engine.shared.sub_agent_count.store(1, Ordering::SeqCst);
    let err = engine
        .fork(AgentType::ChapterCraftAnalyzer, "分析第31章".into())
        .await
        .unwrap_err();
    assert!(matches!(err, AgentError::NestedForkProhibited));
}

#[rstest]
#[tokio::test]
async fn nested_fork_prohibited() {
    let tmp = TempDir::new().unwrap();
    std::fs::create_dir_all(tmp.path().join("skills")).unwrap();
    let engine = AgentEngine::new(test_config(&tmp)).unwrap();
    let child = engine
        .fork(AgentType::KnowledgeAuditor, "审计第31章".into())
        .await
        .unwrap();
    assert!(child.is_child);
}

#[rstest]
#[tokio::test]
async fn fork_empty_task_errors() {
    let tmp = TempDir::new().unwrap();
    std::fs::create_dir_all(tmp.path().join("skills")).unwrap();
    let engine = AgentEngine::new(test_config(&tmp)).unwrap();
    let err = engine
        .fork(AgentType::KnowledgeAuditor, "  ".into())
        .await
        .unwrap_err();
    assert!(matches!(err, AgentError::Fork(ForkError::EmptyTask)));
}

#[rstest]
#[tokio::test]
async fn engine_run_handles_message() {
    let _offline = crate::test_env::StripDeepseekApiKey::new();
    let tmp = TempDir::new().unwrap();
    std::fs::create_dir_all(tmp.path().join("skills")).unwrap();
    let engine = AgentEngine::new(test_config(&tmp)).unwrap();
    let (op_tx, op_rx) = mpsc::unbounded_channel();
    let (event_tx, mut event_rx) = mpsc::unbounded_channel();
    op_tx
        .send(Op::SendMessage {
            content: "你好".into(),
            model: None,
        })
        .unwrap();
    drop(op_tx);
    let handle = tokio::spawn(async move { engine.run(op_rx, event_tx).await });
    let mut saw_turn = false;
    while let Some(ev) = event_rx.recv().await {
        if matches!(ev, Event::TurnComplete { .. }) {
            saw_turn = true;
        }
    }
    assert!(saw_turn);
    assert!(handle.await.unwrap().is_ok());
}

#[rstest]
#[tokio::test]
async fn engine_run_interrupt_op() {
    let tmp = TempDir::new().unwrap();
    std::fs::create_dir_all(tmp.path().join("skills")).unwrap();
    let engine = AgentEngine::new(test_config(&tmp)).unwrap();
    let (op_tx, op_rx) = mpsc::unbounded_channel();
    let (event_tx, _event_rx) = mpsc::unbounded_channel();
    op_tx.send(Op::Interrupt).unwrap();
    drop(op_tx);
    let reason = engine.run(op_rx, event_tx).await.unwrap();
    assert!(matches!(reason, TerminalReason::AbortedStreaming));
}

#[rstest]
#[tokio::test]
async fn engine_run_deny_tool_op() {
    let _offline = crate::test_env::StripDeepseekApiKey::new();
    let tmp = TempDir::new().unwrap();
    std::fs::create_dir_all(tmp.path().join("skills")).unwrap();
    let mut engine = AgentEngine::new(test_config(&tmp)).unwrap();
    engine.pending_tools.insert(
        "p2".into(),
        novel_tools::ToolCallSpec {
            id: "p2".into(),
            name: "Write".into(),
            input: serde_json::json!({"file_path": "settings.json", "content": "x"}),
        },
    );
    let (op_tx, op_rx) = mpsc::unbounded_channel();
    let (event_tx, _event_rx) = mpsc::unbounded_channel();
    op_tx
        .send(Op::DenyTool {
            tool_call_id: "p2".into(),
            reason: Some("no".into()),
        })
        .unwrap();
    drop(op_tx);
    let reason = engine.run(op_rx, event_tx).await.unwrap();
    assert!(matches!(reason, TerminalReason::Completed));
}

#[rstest]
#[tokio::test]
async fn engine_run_resume_session_mismatch_errors() {
    let tmp = TempDir::new().unwrap();
    std::fs::create_dir_all(tmp.path().join("skills")).unwrap();
    let engine = AgentEngine::new(test_config(&tmp)).unwrap();
    let (op_tx, op_rx) = mpsc::unbounded_channel();
    let (event_tx, _event_rx) = mpsc::unbounded_channel();
    op_tx
        .send(Op::ResumeSession {
            session_id: "wrong-id".into(),
        })
        .unwrap();
    drop(op_tx);
    let err = engine.run(op_rx, event_tx).await.unwrap_err();
    assert!(matches!(err, AgentError::Validation(_)));
}

#[rstest]
#[tokio::test]
async fn engine_run_approve_tool_op() {
    let _offline = crate::test_env::StripDeepseekApiKey::new();
    let tmp = TempDir::new().unwrap();
    std::fs::create_dir_all(tmp.path().join("skills")).unwrap();
    let mut engine = AgentEngine::new(test_config(&tmp)).unwrap();
    engine.pending_tools.insert(
        "p1".into(),
        novel_tools::ToolCallSpec {
            id: "p1".into(),
            name: "Read".into(),
            input: serde_json::json!({"file_path": "settings.json"}),
        },
    );
    let (op_tx, op_rx) = mpsc::unbounded_channel();
    let (event_tx, _event_rx) = mpsc::unbounded_channel();
    op_tx
        .send(Op::ApproveTool {
            tool_call_id: "p1".into(),
        })
        .unwrap();
    drop(op_tx);
    let reason = engine.run(op_rx, event_tx).await.unwrap();
    assert!(matches!(reason, TerminalReason::Completed));
}

#[rstest]
#[tokio::test]
async fn resume_session_loads_messages() {
    let _offline = crate::test_env::StripDeepseekApiKey::new();
    let tmp = TempDir::new().unwrap();
    std::fs::create_dir_all(tmp.path().join("skills")).unwrap();
    let cfg = test_config(&tmp);
    let mut engine = AgentEngine::new(cfg.clone()).unwrap();
    engine.handle_message("第一条").await.unwrap();
    let sid = engine.shared.session.id.clone();
    let resumed = AgentEngine::resume(cfg, &sid).unwrap();
    assert!(resumed.messages.len() >= 2);
}

#[test]
fn permission_mode_label_maps_all_modes() {
    use novel_tools::PermissionMode;
    assert_eq!(PermissionMode::Normal.label(), "normal");
    assert_eq!(PermissionMode::Auto.label(), "auto");
    assert_eq!(PermissionMode::Plan.label(), "plan");
    assert_eq!(PermissionMode::Unattended.label(), "unattended");
}

#[test]
fn apply_permission_mode_change_when_idle() {
    use novel_tools::PermissionMode;

    let tmp = TempDir::new().unwrap();
    std::fs::create_dir_all(tmp.path().join("skills")).unwrap();
    let mut engine = AgentEngine::new(test_config(&tmp)).unwrap();
    engine
        .apply_permission_mode_change(PermissionMode::Auto)
        .unwrap();
    assert_eq!(
        engine.tool_context().effective_permission_mode(),
        PermissionMode::Auto
    );
    engine
        .apply_permission_mode_change(PermissionMode::Auto)
        .unwrap();
}

#[test]
fn apply_permission_mode_change_rejects_during_pending_tool() {
    use novel_tools::PermissionMode;

    let tmp = TempDir::new().unwrap();
    std::fs::create_dir_all(tmp.path().join("skills")).unwrap();
    let mut engine = AgentEngine::new(test_config(&tmp)).unwrap();
    engine.pending_tools.insert(
        "tc".into(),
        novel_tools::ToolCallSpec {
            id: "tc".into(),
            name: "Read".into(),
            input: serde_json::json!({"file_path": "a.md"}),
        },
    );
    let err = engine
        .apply_permission_mode_change(PermissionMode::Unattended)
        .unwrap_err();
    assert!(matches!(err, AgentError::Validation(_)));
}

#[test]
fn new_session_persists_permission_mode_to_metadata() {
    let tmp = TempDir::new().unwrap();
    std::fs::create_dir_all(tmp.path().join("skills")).unwrap();
    let engine = AgentEngine::new(test_config(&tmp)).unwrap();
    assert_eq!(
        engine
            .shared
            .session
            .db
            .get_session_permission_mode(&engine.shared.session.id)
            .unwrap()
            .as_deref(),
        Some("normal")
    );
}

#[test]
fn apply_permission_mode_change_persists_and_resume_restores() {
    use novel_tools::PermissionMode;

    let tmp = TempDir::new().unwrap();
    std::fs::create_dir_all(tmp.path().join("skills")).unwrap();
    let cfg = test_config(&tmp);
    let mut engine = AgentEngine::new(cfg.clone()).unwrap();
    engine
        .apply_permission_mode_change(PermissionMode::Plan)
        .unwrap();
    let sid = engine.shared.session.id.clone();
    assert_eq!(
        engine
            .shared
            .session
            .db
            .get_session_permission_mode(&sid)
            .unwrap()
            .as_deref(),
        Some("plan")
    );
    let resumed = AgentEngine::resume(cfg, &sid).unwrap();
    assert_eq!(
        resumed.tool_context().effective_permission_mode(),
        PermissionMode::Plan
    );
}

#[test]
fn resume_metadata_auto_without_switch() {
    use novel_tools::PermissionMode;

    let tmp = TempDir::new().unwrap();
    std::fs::create_dir_all(tmp.path().join("skills")).unwrap();
    let cfg = test_config(&tmp);
    let engine = AgentEngine::new(cfg.clone()).unwrap();
    let sid = engine.shared.session.id.clone();
    engine
        .shared
        .session
        .db
        .set_session_permission_mode(&sid, "auto")
        .unwrap();
    let resumed = AgentEngine::resume(cfg, &sid).unwrap();
    assert_eq!(
        resumed.tool_context().effective_permission_mode(),
        PermissionMode::Auto
    );
    assert!(resumed.pending_permission_user_prefix.is_none());
}

#[test]
fn normal_to_unattended_sets_dual_pending_prefix() {
    use crate::permission::{AUTONOMOUS_MODE_MARKER, PERMISSION_MODE_ENTER_PREFIX};
    use novel_tools::PermissionMode;

    let tmp = TempDir::new().unwrap();
    std::fs::create_dir_all(tmp.path().join("skills")).unwrap();
    let mut engine = AgentEngine::new(test_config(&tmp)).unwrap();
    engine
        .apply_permission_mode_change(PermissionMode::Unattended)
        .unwrap();
    let prefix = engine
        .pending_permission_user_prefix
        .as_ref()
        .expect("dual inject");
    assert!(prefix.starts_with(PERMISSION_MODE_ENTER_PREFIX));
    assert!(prefix.contains(AUTONOMOUS_MODE_MARKER));
}

#[test]
fn resume_unattended_switch_normal_sets_exit_prefix() {
    use crate::permission::PERMISSION_MODE_EXIT_PREFIX;
    use novel_tools::PermissionMode;

    let tmp = TempDir::new().unwrap();
    std::fs::create_dir_all(tmp.path().join("skills")).unwrap();
    let cfg = test_config(&tmp);
    let mut engine = AgentEngine::new(cfg.clone()).unwrap();
    engine
        .apply_permission_mode_change(PermissionMode::Unattended)
        .unwrap();
    let sid = engine.shared.session.id.clone();
    let mut resumed = AgentEngine::resume(cfg, &sid).unwrap();
    assert_eq!(
        resumed.tool_context().effective_permission_mode(),
        PermissionMode::Unattended
    );
    resumed
        .apply_permission_mode_change(PermissionMode::Normal)
        .unwrap();
    let prefix = resumed
        .pending_permission_user_prefix
        .as_ref()
        .expect("exit inject");
    assert!(prefix.starts_with(PERMISSION_MODE_EXIT_PREFIX));
}

#[test]
fn plan_to_auto_switch_no_pending_prefix() {
    use novel_tools::PermissionMode;

    let tmp = TempDir::new().unwrap();
    std::fs::create_dir_all(tmp.path().join("skills")).unwrap();
    let mut engine = AgentEngine::new(test_config(&tmp)).unwrap();
    engine
        .apply_permission_mode_change(PermissionMode::Plan)
        .unwrap();
    engine
        .apply_permission_mode_change(PermissionMode::Auto)
        .unwrap();
    assert!(engine.pending_permission_user_prefix.is_none());
}

#[rstest]
#[tokio::test]
async fn mode_switch_then_send_merges_prefix_and_display_content() {
    use crate::message::stored_message_display_text;
    use crate::permission::{PERMISSION_MODE_ENTER_PREFIX, USER_CONTENT_SEPARATOR};
    use novel_tools::PermissionMode;

    let _offline = crate::test_env::StripDeepseekApiKey::new();
    let tmp = TempDir::new().unwrap();
    std::fs::create_dir_all(tmp.path().join("skills")).unwrap();
    let cfg = test_config(&tmp);
    let mut engine = AgentEngine::new(cfg.clone()).unwrap();
    engine
        .apply_permission_mode_change(PermissionMode::Unattended)
        .unwrap();
    engine.handle_message("写第3章").await.unwrap();
    let sid = engine.shared.session.id.clone();
    let msgs = engine
        .shared
        .session
        .db
        .get_session_messages(&sid, Some((1, 1)))
        .unwrap();
    let user = msgs
        .iter()
        .find(|m| m.role == "user" && m.turn_number == 1)
        .expect("turn 1 user message");
    let json = &user.content_json;
    let content = json.get("content").and_then(|v| v.as_str()).unwrap();
    assert!(content.starts_with(PERMISSION_MODE_ENTER_PREFIX));
    assert!(content.contains(USER_CONTENT_SEPARATOR));
    assert!(content.ends_with("写第3章"));
    assert_eq!(stored_message_display_text(json), "写第3章");
}

#[test]
fn clear_read_file_cache_removes_all_entries() {
    use novel_tools::{ReadCacheEntry, ReadCacheSource};
    use std::path::PathBuf;

    let tmp = TempDir::new().unwrap();
    std::fs::create_dir_all(tmp.path().join("skills")).unwrap();
    let engine = AgentEngine::new(test_config(&tmp)).unwrap();
    engine.shared.read_file_cache.insert(
        PathBuf::from("a.md"),
        ReadCacheEntry {
            mtime_secs: 1,
            raw_content: "x".into(),
            offset: None,
            limit: None,
            total_lines: 1,
            source: ReadCacheSource::WriteRefresh,
            transcript_committed: true,
            committed_spans: Vec::new(),
            committed_offset: None,
            committed_limit: None,
        },
    );
    assert_eq!(engine.shared.read_file_cache.len(), 1);
    engine.shared.clear_read_file_cache();
    assert!(engine.shared.read_file_cache.is_empty());
}
