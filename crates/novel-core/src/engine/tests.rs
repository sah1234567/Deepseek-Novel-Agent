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
        },
    );
    assert_eq!(engine.shared.read_file_cache.len(), 1);
    engine.shared.clear_read_file_cache();
    assert!(engine.shared.read_file_cache.is_empty());
}
