//! Integration tests for read-cache sync (resume hydrate / rebuild).

use crate::engine::AgentEngine;
use novel_tools::ReadCacheSource;
use tempfile::TempDir;

fn test_engine_config(tmp: &TempDir) -> crate::EngineConfig {
    std::fs::create_dir_all(tmp.path().join("skills")).unwrap();
    crate::EngineConfig {
        project_root: tmp.path().to_path_buf(),
        settings_path: tmp.path().join("settings.json"),
        db_path: tmp.path().join("state.db"),
        skills_dir: tmp.path().join("skills"),
        global_config_path: tmp.path().join(".novel-agent/api_config.json"),
    }
}

#[test]
fn resume_rebuilds_read_cache_from_transcript() {
    let tmp = TempDir::new().unwrap();
    let file = tmp.path().join("note.md");
    let lines: String = (1..=40)
        .map(|i| format!("line {i}"))
        .collect::<Vec<_>>()
        .join("\n");
    std::fs::write(&file, &lines).unwrap();

    let config = test_engine_config(&tmp);
    let mut engine = AgentEngine::new(config.clone()).unwrap();
    let sid = engine.shared.session.id.clone();

    // Simulate Read + tool_result in transcript via offline path is heavy; insert messages directly.
    use crate::message::{chat_to_json, tool_result_message};
    use crate::{ChatMessage, ToolCallRecord};

    engine.messages.push(ChatMessage {
        role: "user".into(),
        content: "read".into(),
        ..Default::default()
    });
    engine.turn_number = 1;
    engine.messages.push(ChatMessage {
        role: "assistant".into(),
        content: String::new(),
        tool_calls: Some(vec![ToolCallRecord {
            id: "r1".into(),
            name: "Read".into(),
            arguments: serde_json::json!({
                "file_path": "note.md",
                "offset": 10,
                "limit": 5
            }),
        }]),
        ..Default::default()
    });
    engine
        .messages
        .push(tool_result_message("r1", "10\tline 10"));
    for (i, msg) in engine.messages.iter().enumerate() {
        if i == 0 {
            continue;
        }
        let seq = i as i32;
        engine
            .shared
            .session
            .db
            .insert_message(&sid, 1, seq, &msg.role, &chat_to_json(msg), None)
            .unwrap();
    }

    let resumed = AgentEngine::resume(config, &sid).unwrap();
    let full = resumed.shared.session.project_root.join("note.md");
    assert!(
        resumed.shared.read_file_cache.contains_key(&full),
        "resume should rebuild read cache from transcript"
    );
    let entry = resumed.shared.read_file_cache.get(&full).unwrap();
    assert_eq!(entry.source, ReadCacheSource::Read);
    assert!(entry.transcript_committed);
}

#[test]
fn resume_hydrates_from_session_read_cache_subtable() {
    let tmp = TempDir::new().unwrap();
    std::fs::write(tmp.path().join("note.md"), "hello\nworld\n").unwrap();

    let config = test_engine_config(&tmp);
    let engine = AgentEngine::new(config.clone()).unwrap();
    let sid = engine.shared.session.id.clone();

    let entry = novel_tools::ReadCacheEntry {
        mtime_secs: 1,
        raw_content: "hello\nworld".into(),
        offset: None,
        limit: None,
        total_lines: 2,
        source: ReadCacheSource::Read,
        transcript_committed: true,
        committed_spans: vec![],
        committed_offset: None,
        committed_limit: None,
    };
    let json = serde_json::to_string(&entry).unwrap();
    engine
        .shared
        .session
        .db
        .upsert_session_read_cache_entry(&sid, "note.md", &json)
        .unwrap();
    engine
        .shared
        .session
        .db
        .set_read_cache_anchor(
            &sid,
            &novel_state::ReadCacheAnchor {
                compaction_count: 0,
                anchor_turn: 0,
                anchor_sequence: 0,
            },
        )
        .unwrap();

    let resumed = AgentEngine::resume(config, &sid).unwrap();
    let full = resumed.shared.session.project_root.join("note.md");
    assert!(resumed.shared.read_file_cache.contains_key(&full));
}
