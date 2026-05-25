//! Live turn test: real DeepSeek API + multiple InvokeSkill tool calls.
//!
//! Uses `deepseek-v4-pro` with thinking enabled (default). Verifies messages DB rows
//! stay unique on `(session_id, turn_number, sequence)`.
//!
//! Run (PowerShell):
//!   $env:DEEPSEEK_API_KEY = "sk-..."
//!   cargo nextest run -p novel-core --test live_invoke_skill_turn --run-ignored all

use novel_core::{AgentEngine, EngineConfig};
use std::collections::HashSet;
use std::time::Duration;
use tempfile::TempDir;

fn test_config(tmp: &TempDir) -> EngineConfig {
    let settings_path = tmp.path().join("settings.json");
    std::fs::write(
        &settings_path,
        r#"{"model":{"model":"deepseek-v4-pro","thinking_enabled":true}}"#,
    )
    .unwrap();
    EngineConfig {
        project_root: tmp.path().to_path_buf(),
        settings_path,
        db_path: tmp.path().join("state.db"),
        skills_dir: tmp.path().join("skills"),
        global_config_path: tmp.path().join(".novel-agent/api_config.json"),
    }
}

fn write_skill(skills_root: &std::path::Path, id: &str) {
    let dir = skills_root.join(id);
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(
        dir.join("SKILL.md"),
        format!(
            "---\nname: {id}\ndescription: live test skill\nwhen_to_use: test\n---\n# {id}\n"
        ),
    )
    .unwrap();
}

fn assert_unique_turn_sequences(
    stored: &[novel_state::StoredMessage],
    session_id: &str,
) {
    let mut seen = HashSet::new();
    for m in stored {
        assert_eq!(
            m.session_id, session_id,
            "unexpected session_id on message {}",
            m.id
        );
        assert!(
            seen.insert((m.turn_number, m.sequence)),
            "duplicate (turn_number, sequence)=({}, {}) role={}",
            m.turn_number,
            m.sequence,
            m.role
        );
    }
}

#[tokio::test]
#[ignore = "requires DEEPSEEK_API_KEY and network"]
async fn live_turn_invoke_skills_no_duplicate_db_rows() {
    let _ = std::env::var("DEEPSEEK_API_KEY").expect("set DEEPSEEK_API_KEY");

    let tmp = TempDir::new().unwrap();
    let skills = tmp.path().join("skills");
    std::fs::create_dir_all(&skills).unwrap();
    for id in ["novel-planning", "rebirth", "power-fantasy"] {
        write_skill(&skills, id);
    }

    let config = test_config(&tmp);
    let mut engine = AgentEngine::new(config).unwrap();
    assert_eq!(
        engine.shared.settings.model.model, "deepseek-v4-pro",
        "test must run against deepseek-v4-pro"
    );
    assert!(
        engine.shared.settings.model.thinking_enabled,
        "deepseek-v4-pro live test expects thinking_enabled"
    );

    let session_id = engine.shared.session.id.clone();

    let prompt = "请在本轮依次调用 InvokeSkill，skill_id 分别为 novel-planning、rebirth、power-fantasy \
                  （各调用一次，不要跳过）。完成后用一句话确认三个 skill 已加载。";

    let result = tokio::time::timeout(
        Duration::from_secs(300),
        engine.handle_message(prompt),
    )
    .await
    .expect("turn timed out after 300s")
    .expect("handle_message should not return State/DB error");

    println!("terminal reason: {result:?}");

    let stored = engine
        .shared
        .session
        .db
        .get_session_messages(&session_id, None)
        .expect("load messages");

    assert_unique_turn_sequences(&stored, &session_id);

    let tool_rows: Vec<_> = stored.iter().filter(|m| m.role == "tool").collect();
    println!("persisted messages: {}, tool results: {}", stored.len(), tool_rows.len());
    assert!(
        !tool_rows.is_empty(),
        "expected at least one InvokeSkill tool result in DB"
    );
}
