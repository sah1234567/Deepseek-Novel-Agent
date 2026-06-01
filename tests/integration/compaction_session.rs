#![allow(clippy::unwrap_used)]

use novel_core::{AgentEngine, EngineConfig};
use novel_state::Database;
use tempfile::TempDir;

fn test_engine_config(tmp: &TempDir) -> EngineConfig {
    let project = tmp.path().join("project");
    std::fs::create_dir_all(&project).unwrap();
    std::fs::write(project.join("AGENTS.md"), "# agents").unwrap();
    let db_path = tmp.path().join("state.db");
    EngineConfig {
        project_root: project,
        db_path,
        settings_path: tmp.path().join("settings.json"),
        skills_dir: tmp.path().join("skills"),
        global_config_path: tmp.path().join("config.toml"),
    }
}

#[test]
fn new_session_persists_system_at_turn_zero() {
    let tmp = TempDir::new().unwrap();
    std::fs::create_dir_all(tmp.path().join("skills")).unwrap();
    let config = test_engine_config(&tmp);
    let engine = AgentEngine::new(config.clone()).expect("engine");
    let stored = Database::open(&config.db_path)
        .unwrap()
        .get_session_messages(&engine.shared.session.id, None)
        .unwrap();
    assert!(!stored.is_empty());
    assert_eq!(stored[0].turn_number, 0);
    assert_eq!(stored[0].sequence, 0);
    assert_eq!(stored[0].role, "system");
    let meta = Database::open(&config.db_path)
        .unwrap()
        .get_session_metadata(&engine.shared.session.id)
        .unwrap()
        .expect("metadata");
    assert_eq!(
        meta.get("system_static_frozen").and_then(|v| v.as_bool()),
        Some(true)
    );
}

#[test]
fn resume_loads_frozen_system_without_rebuild() {
    let tmp = TempDir::new().unwrap();
    std::fs::create_dir_all(tmp.path().join("skills")).unwrap();
    let config = test_engine_config(&tmp);
    let engine = AgentEngine::new(config.clone()).expect("engine");
    let session_id = engine.shared.session.id.clone();
    let original_system = engine.shared.system_prompt.clone();
    drop(engine);

    let resumed = AgentEngine::resume(config, &session_id).expect("resume");
    assert_eq!(resumed.shared.system_prompt, original_system);
    assert_eq!(resumed.messages[0].content, original_system);
}
