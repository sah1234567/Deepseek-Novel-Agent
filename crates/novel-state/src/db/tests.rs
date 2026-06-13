use super::Database;
use crate::{turn_bounds::TurnBounds, StateError};
use rstest::rstest;
use rusqlite::params;
use std::sync::Arc;
use tempfile::TempDir;

/// Holds `TempDir` for the whole test — dropping it early deletes the DB under concurrency.
struct TestDb {
    _dir: TempDir,
    db: Database,
    path: std::path::PathBuf,
}

impl std::ops::Deref for TestDb {
    type Target = Database;

    fn deref(&self) -> &Self::Target {
        &self.db
    }
}

fn test_db() -> TestDb {
    let tmp = TempDir::new().unwrap();
    let path = tmp.path().join("test.db");
    let db = Database::open(&path).unwrap();
    TestDb {
        _dir: tmp,
        db,
        path,
    }
}

#[test]
fn upsert_session_todos_replace_and_merge() {
    let db = test_db();
    let sid = db.create_session("/tmp/proj", "deepseek-chat").unwrap();
    let t1 = crate::SessionTodo {
        id: "1".into(),
        content: "a".into(),
        status: "pending".into(),
    };
    db.upsert_session_todos(&sid, std::slice::from_ref(&t1), false)
        .unwrap();
    assert_eq!(db.list_session_todos(&sid).unwrap().len(), 1);
    let t2 = crate::SessionTodo {
        id: "2".into(),
        content: "b".into(),
        status: "done".into(),
    };
    db.upsert_session_todos(&sid, &[t2], true).unwrap();
    let listed = db.list_session_todos(&sid).unwrap();
    assert_eq!(listed.len(), 2);
    db.upsert_session_todos(&sid, &[], false).unwrap();
    assert!(db.list_session_todos(&sid).unwrap().is_empty());
}

#[test]
fn session_todos_keep_creation_order_when_status_changes() {
    let db = test_db();
    let sid = db.create_session("/tmp/proj", "deepseek-chat").unwrap();
    db.upsert_session_todos(
        &sid,
        &[
            crate::SessionTodo {
                id: "1".into(),
                content: "first".into(),
                status: "pending".into(),
            },
            crate::SessionTodo {
                id: "2".into(),
                content: "second".into(),
                status: "pending".into(),
            },
            crate::SessionTodo {
                id: "3".into(),
                content: "third".into(),
                status: "pending".into(),
            },
        ],
        false,
    )
    .unwrap();
    db.upsert_session_todos(
        &sid,
        &[crate::SessionTodo {
            id: "2".into(),
            content: "second".into(),
            status: "in_progress".into(),
        }],
        true,
    )
    .unwrap();
    let listed = db.list_session_todos(&sid).unwrap();
    assert_eq!(
        listed.iter().map(|t| t.id.as_str()).collect::<Vec<_>>(),
        vec!["1", "2", "3"]
    );
    assert_eq!(listed[1].status, "in_progress");
}

#[rstest]
#[test]
fn migration_creates_tables() {
    let db = test_db();
    let tables = db.list_tables().unwrap();
    assert!(tables.contains(&"sessions".to_string()));
    assert!(tables.contains(&"messages".to_string()));
    assert!(tables.contains(&"fork_runs".to_string()));
    assert!(tables.contains(&"fork_messages".to_string()));
    assert!(tables.contains(&"session_todos".to_string()));
    assert!(tables.contains(&"message_archive".to_string()));
    assert!(!tables.contains(&"checkpoints".to_string()));
    assert!(!tables.contains(&"sub_agent_runs".to_string()));
}

#[test]
fn archive_session_messages_preserves_epoch() {
    let db = test_db();
    let sid = db.create_session("/tmp/proj", "deepseek-chat").unwrap();
    db.insert_message(
        &sid,
        0,
        0,
        "system",
        &serde_json::json!({"role":"system","content":"sys"}),
        None,
    )
    .unwrap();
    db.insert_message(
        &sid,
        1,
        0,
        "user",
        &serde_json::json!({"role":"user","content":"hello"}),
        None,
    )
    .unwrap();
    db.archive_session_messages(&sid, 1).unwrap();
    let epochs = db.get_archived_epochs(&sid).unwrap();
    assert_eq!(epochs, vec![1]);
    let archived = db.get_archived_messages(&sid, 1).unwrap();
    assert_eq!(archived.len(), 2);
    db.replace_session_messages(
        &sid,
        &[(
            0,
            0,
            "system",
            &serde_json::json!({"role":"system","content":"new"}),
        )],
    )
    .unwrap();
    assert_eq!(db.get_session_messages(&sid, None).unwrap().len(), 1);
    assert_eq!(db.get_archived_messages(&sid, 1).unwrap().len(), 2);
}

#[test]
fn compaction_retained_turn_metadata_roundtrip() {
    let tmp = tempfile::TempDir::new().unwrap();
    let db = Database::open(tmp.path().join("test.db")).unwrap();
    let sid = db.create_session("/tmp", "m").unwrap();
    db.record_compaction_retained_turns(&sid, 1, 46, 50)
        .unwrap();
    assert_eq!(
        db.get_compaction_retained_turn_bounds(&sid, 1).unwrap(),
        Some((46, 50))
    );
    assert_eq!(
        db.get_compaction_retained_turn_bounds(&sid, 2).unwrap(),
        None
    );
}

#[test]
fn increment_compaction_count_metadata() {
    let db = test_db();
    let sid = db.create_session("/tmp/proj", "deepseek-chat").unwrap();
    assert_eq!(db.get_compaction_count(&sid).unwrap(), 0);
    assert_eq!(db.increment_compaction_count(&sid).unwrap(), 1);
    assert_eq!(db.get_compaction_count(&sid).unwrap(), 1);
}

#[test]
fn replace_session_messages_swaps_history() {
    let db = test_db();
    let sid = db.create_session("/tmp/proj", "deepseek-chat").unwrap();
    for i in 0..5 {
        db.insert_message(
            &sid,
            i,
            0,
            "user",
            &serde_json::json!({"role":"user","content":format!("msg {i}")}),
            None,
        )
        .unwrap();
    }
    let sys = serde_json::json!({"role":"system","content":"sys"});
    let user = serde_json::json!({"role":"user","content":"compact"});
    let replacement = vec![(0i32, 0i32, "system", &sys), (1i32, 0i32, "user", &user)];
    db.replace_session_messages(&sid, &replacement).unwrap();
    let msgs = db.get_session_messages(&sid, None).unwrap();
    assert_eq!(msgs.len(), 2);
    assert_eq!(msgs[0].role, "system");
    assert_eq!(msgs[1].role, "user");
}

#[test]
fn invoked_skill_ids_metadata_roundtrip() {
    let db = test_db();
    let sid = db.create_session("/tmp/proj", "deepseek-chat").unwrap();
    db.set_invoked_skill_ids(&sid, &["plot-grid".into(), "log-integrity-checker".into()])
        .unwrap();
    let ids = db.get_invoked_skill_ids(&sid).unwrap();
    assert_eq!(ids, vec!["plot-grid", "log-integrity-checker"]);
}

#[test]
fn permission_mode_metadata_roundtrip() {
    let db = test_db();
    let sid = db.create_session("/tmp/proj", "deepseek-chat").unwrap();
    assert!(db.get_session_permission_mode(&sid).unwrap().is_none());
    db.set_session_permission_mode(&sid, "unattended").unwrap();
    assert_eq!(
        db.get_session_permission_mode(&sid).unwrap().as_deref(),
        Some("unattended")
    );
}

#[test]
fn set_permission_mode_preserves_other_metadata_keys() {
    let db = test_db();
    let sid = db.create_session("/tmp/proj", "deepseek-chat").unwrap();
    db.set_invoked_skill_ids(&sid, &["plot-grid".into()])
        .unwrap();
    db.set_session_permission_mode(&sid, "plan").unwrap();
    assert_eq!(
        db.get_invoked_skill_ids(&sid).unwrap(),
        vec!["plot-grid".to_string()]
    );
    assert_eq!(
        db.get_session_permission_mode(&sid).unwrap().as_deref(),
        Some("plan")
    );
}

#[test]
fn read_skill_reference_paths_metadata_roundtrip() {
    let db = test_db();
    let sid = db.create_session("/tmp/proj", "deepseek-chat").unwrap();
    assert!(db.get_read_skill_reference_paths(&sid).unwrap().is_empty());
    db.set_read_skill_reference_paths(
        &sid,
        &[
            "apocalypse/references/zombie.md".into(),
            "romance/references/harem.md".into(),
        ],
    )
    .unwrap();
    let paths = db.get_read_skill_reference_paths(&sid).unwrap();
    assert_eq!(
        paths,
        vec![
            "apocalypse/references/zombie.md",
            "romance/references/harem.md"
        ]
    );
}

#[test]
fn session_crud() {
    let db = test_db();
    let id = db.create_session("/tmp/proj", "deepseek-chat").unwrap();
    let s = db.get_session(&id).unwrap().unwrap();
    assert_eq!(s.status, "active");
    db.update_session_status(&id, "completed").unwrap();
    let s2 = db.get_session(&id).unwrap().unwrap();
    assert_eq!(s2.status, "completed");
}

#[test]
fn get_nonexistent_session() {
    let db = test_db();
    assert!(db.get_session("nonexistent").unwrap().is_none());
}

#[test]
fn token_accumulates_for_billing() {
    let db = test_db();
    let sid = db.create_session("/tmp/proj", "deepseek-chat").unwrap();
    db.accumulate_session_tokens(&sid, 100, 50, 30, "deepseek-v4-pro", true)
        .unwrap();
    db.accumulate_session_tokens(&sid, 200, 80, 70, "deepseek-v4-pro", true)
        .unwrap();
    let s = db.get_session(&sid).unwrap().unwrap();
    assert_eq!(s.cache_hit_tokens, 300);
    assert_eq!(s.cache_miss_tokens, 130);
    assert_eq!(s.completion_tokens, 100);
    assert_eq!(s.api_call_count, 2);
    assert_eq!(s.total_turns, 0);
    assert_eq!(s.model, "deepseek-v4-pro");
    assert_eq!(s.context_tokens, 350);
}

#[test]
fn accumulate_session_tokens_skips_context_snapshot_when_disabled() {
    let db = test_db();
    let sid = db.create_session("/tmp/proj", "deepseek-chat").unwrap();
    db.accumulate_session_tokens(&sid, 100, 50, 30, "deepseek-v4-pro", true)
        .unwrap();
    assert_eq!(db.get_session(&sid).unwrap().unwrap().context_tokens, 180);
    db.accumulate_session_tokens(&sid, 5, 3, 2, "deepseek-v4-pro", false)
        .unwrap();
    let s = db.get_session(&sid).unwrap().unwrap();
    assert_eq!(s.context_tokens, 180);
    assert_eq!(s.cache_hit_tokens, 105);
    assert_eq!(s.api_call_count, 2);
}

#[test]
fn sync_user_turn_count_does_not_touch_last_active_at() {
    let db = test_db();
    let sid = db.create_session("/tmp/proj", "deepseek-chat").unwrap();
    let before = db.get_session(&sid).unwrap().unwrap().last_active_at;
    std::thread::sleep(std::time::Duration::from_millis(10));
    db.sync_user_turn_count(&sid, 3).unwrap();
    let after = db.get_session(&sid).unwrap().unwrap().last_active_at;
    assert_eq!(after, before);
}

#[test]
fn accumulate_session_tokens_updates_last_active_at() {
    let db = test_db();
    let sid = db.create_session("/tmp/proj", "deepseek-chat").unwrap();
    let before = db.get_session(&sid).unwrap().unwrap().last_active_at;
    std::thread::sleep(std::time::Duration::from_millis(10));
    db.accumulate_session_tokens(&sid, 1, 0, 0, "deepseek-v4-pro", true)
        .unwrap();
    let after = db.get_session(&sid).unwrap().unwrap().last_active_at;
    assert!(after > before);
}

#[test]
fn user_turn_count_independent_of_api_calls() {
    let db = test_db();
    let sid = db.create_session("/tmp/proj", "deepseek-chat").unwrap();
    db.sync_user_turn_count(&sid, 1).unwrap();
    db.accumulate_session_tokens(&sid, 10, 5, 2, "deepseek-v4-pro", true)
        .unwrap();
    db.accumulate_session_tokens(&sid, 10, 5, 2, "deepseek-v4-pro", true)
        .unwrap();
    db.accumulate_session_tokens(&sid, 10, 5, 2, "deepseek-v4-pro", true)
        .unwrap();
    let s = db.get_session(&sid).unwrap().unwrap();
    assert_eq!(s.total_turns, 1);
    assert_eq!(s.api_call_count, 3);
}

#[test]
fn message_insert_and_query() {
    let db = test_db();
    let sid = db.create_session("/tmp/proj", "deepseek-chat").unwrap();
    db.insert_message(
        &sid,
        1,
        0,
        "user",
        &serde_json::json!({"content":"hello"}),
        None,
    )
    .unwrap();
    let msgs = db.get_session_messages(&sid, None).unwrap();
    assert_eq!(msgs.len(), 1);
    assert_eq!(msgs[0].role, "user");
}

#[test]
fn active_turn_bounds_empty_and_populated() {
    let db = test_db();
    let sid = db.create_session("/tmp/proj", "deepseek-chat").unwrap();
    assert_eq!(db.get_active_turn_bounds(&sid).unwrap(), None);
    db.insert_message(
        &sid,
        1,
        0,
        "user",
        &serde_json::json!({"content":"hello"}),
        None,
    )
    .unwrap();
    db.insert_message(
        &sid,
        3,
        0,
        "user",
        &serde_json::json!({"content":"later"}),
        None,
    )
    .unwrap();
    assert_eq!(
        db.get_active_turn_bounds(&sid).unwrap(),
        Some(TurnBounds::new(1, 3))
    );
}

#[test]
fn archived_turn_bounds_and_range() {
    let db = test_db();
    let sid = db.create_session("/tmp/proj", "deepseek-chat").unwrap();
    for t in 1..=3 {
        db.insert_message(
            &sid,
            t,
            0,
            "user",
            &serde_json::json!({"content":format!("t{t}")}),
            None,
        )
        .unwrap();
    }
    db.archive_session_messages(&sid, 1).unwrap();
    assert_eq!(
        db.get_archived_turn_bounds(&sid, 1).unwrap(),
        Some(TurnBounds::new(1, 3))
    );
    let slice = db
        .get_archived_messages_turn_range(&sid, 1, Some((2, 2)))
        .unwrap();
    assert_eq!(slice.len(), 1);
    assert_eq!(slice[0].turn_number, 2);
}

#[test]
fn has_active_context_refresh_detects_turn_zero_user() {
    let db = test_db();
    let sid = db.create_session("/tmp/proj", "deepseek-chat").unwrap();
    assert!(!db.has_active_context_refresh(&sid).unwrap());
    db.insert_message(
        &sid,
        0,
        1,
        "user",
        &serde_json::json!({"content":"[上下文刷新]\n## 会话历史摘要\nx"}),
        None,
    )
    .unwrap();
    assert!(db.has_active_context_refresh(&sid).unwrap());
}

#[test]
fn message_query_turn_range() {
    let db = test_db();
    let sid = db.create_session("/tmp/proj", "deepseek-chat").unwrap();
    for t in 1..=5 {
        db.insert_message(
            &sid,
            t,
            0,
            "user",
            &serde_json::json!({"content":format!("t{t}")}),
            None,
        )
        .unwrap();
    }
    let msgs = db.get_session_messages(&sid, Some((2, 4))).unwrap();
    assert_eq!(msgs.len(), 3);
}

#[test]
fn list_sessions_filters_by_project_and_orders_by_activity() {
    let db = test_db();
    let a = db.create_session("/proj/a", "deepseek-chat").unwrap();
    let b = db.create_session("/proj/b", "deepseek-chat").unwrap();
    let c = db.create_session("/proj/a", "deepseek-chat").unwrap();
    db.accumulate_session_tokens(&a, 1, 0, 0, "deepseek-v4-pro", true)
        .unwrap();
    db.accumulate_session_tokens(&b, 1, 0, 0, "deepseek-v4-pro", true)
        .unwrap();
    db.accumulate_session_tokens(&c, 1, 0, 0, "deepseek-v4-pro", true)
        .unwrap();
    let list = db.list_sessions("/proj/a", 10).unwrap();
    assert_eq!(list.len(), 2);
    // Most recent first
    assert_eq!(list[0].id, c);
}

#[test]
fn database_recreated_if_deleted() {
    let tmp = TempDir::new().unwrap();
    let p = tmp.path().join("test.db");
    {
        let db = Database::open(&p).unwrap();
        let id = db.create_session("/proj", "deepseek-chat").unwrap();
        db.accumulate_session_tokens(&id, 10, 5, 2, "deepseek-v4-pro", true)
            .unwrap();
    }
    std::fs::remove_file(&p).unwrap();
    let db = Database::open(&p).unwrap();
    assert_eq!(db.session_count().unwrap(), 0);
}

#[test]
fn fork_run_messages_persist_in_order() {
    let db = test_db();
    let sid = db.create_session("/tmp/proj", "deepseek-chat").unwrap();
    let run_id = db
        .create_fork_run(&sid, 1, "KnowledgeAuditor", "audit ch1", "tool")
        .unwrap();
    db.insert_fork_message(
        &run_id,
        "assistant",
        &serde_json::json!({"content": "checking"}),
    )
    .unwrap();
    db.insert_fork_message(
        &run_id,
        "tool",
        &serde_json::json!({"content": "ok", "tool_call_id": "tc1"}),
    )
    .unwrap();
    let msgs = db.get_fork_messages(&run_id).unwrap();
    assert_eq!(msgs.len(), 2);
    assert_eq!(msgs[0].sequence, 0);
    assert_eq!(msgs[1].sequence, 1);
    db.finish_fork_run(&run_id, "complete", Some("report-id"))
        .unwrap();
    let msgs_after = db.get_fork_messages(&run_id).unwrap();
    assert_eq!(msgs_after.len(), 2);
}

#[test]
fn get_fork_messages_rejects_corrupt_content_json() {
    let guard = test_db();
    let db = &guard.db;
    let sid = db.create_session("/tmp/proj", "deepseek-chat").unwrap();
    let run_id = db
        .create_fork_run(&sid, 1, "KnowledgeAuditor", "audit", "tool")
        .unwrap();
    let conn = rusqlite::Connection::open(&guard.path).unwrap();
    conn.execute(
        "INSERT INTO fork_messages (id, run_id, sequence, role, content_json, created_at)
         VALUES (?1, ?2, 0, 'assistant', ?3, ?4)",
        params![
            "bad-fork-msg",
            run_id,
            "{not valid json",
            chrono::Utc::now().to_rfc3339(),
        ],
    )
    .unwrap();
    let err = db.get_fork_messages(&run_id).unwrap_err();
    assert!(matches!(err, StateError::Json(_)));
}

#[test]
fn concurrent_writes() {
    let guard = test_db();
    let db = Arc::new(guard.db.clone());
    let sid = db.create_session("/proj", "deepseek-chat").unwrap();
    let mut handles = vec![];
    for _ in 0..10 {
        let db = Arc::clone(&db);
        let sid = sid.clone();
        handles.push(std::thread::spawn(move || {
            db.accumulate_session_tokens(&sid, 1, 0, 0, "deepseek-v4-pro", true)
                .expect("concurrent accumulate_session_tokens");
        }));
    }
    for h in handles {
        h.join().expect("worker thread panicked");
    }
    let s = db.get_session(&sid).unwrap().unwrap();
    // Tokens replace (race — last write wins), API calls still increment
    assert!(s.cache_hit_tokens >= 1);
    assert_eq!(s.api_call_count, 10);
    assert_eq!(s.total_turns, 0);
}
