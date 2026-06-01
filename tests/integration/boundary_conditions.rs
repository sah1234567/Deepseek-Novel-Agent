//! Integration tests covering FRAMEWORK.md §10 boundary conditions.

#![allow(clippy::unwrap_used)]

use novel_config::load_project_settings;
use novel_core::{AgentEngine, AgentType, EngineConfig, ForkError};
use novel_knowledge::{append_evolution_log, parse_causality_markdown, KnowledgeStore};
use novel_skills::load_skills_dir;
use novel_state::Database;
use tempfile::TempDir;

fn engine_config(tmp: &TempDir) -> EngineConfig {
    EngineConfig {
        project_root: tmp.path().join("novels/demo"),
        settings_path: tmp.path().join("settings.json"),
        db_path: tmp.path().join("state.db"),
        skills_dir: tmp.path().join("skills"),
        global_config_path: tmp.path().join(".novel-agent/api_config.json"),
    }
}

#[test]
fn settings_invalid_compaction_threshold() {
    let tmp = TempDir::new().unwrap();
    std::fs::write(
        tmp.path().join("settings.json"),
        r#"{"model":{"compaction_threshold":2.0,"context_window_size":1000}}"#,
    )
    .unwrap();
    assert!(load_project_settings(tmp.path().join("settings.json")).is_err());
}

#[test]
fn knowledge_utf8_only() {
    let tmp = TempDir::new().unwrap();
    let store = KnowledgeStore::new(tmp.path());
    let path = tmp.path().join("bad.bin");
    std::fs::write(&path, [0xFF, 0xFE]).unwrap();
    let err = store.read_file("bad.bin").unwrap_err();
    assert!(matches!(
        err,
        novel_knowledge::KnowledgeError::EncodingError { .. }
    ));
}

#[test]
fn causality_cycle_prevented() {
    let content = r#"
| 章节 | 来源 | 关系 | 目标 | 描述 |
| Ch1 | A | 引发 | B | a |
| Ch1 | B | 导致 | A | b |
"#;
    let graph = parse_causality_markdown(content);
    // second edge should be skipped due to cycle in add_edge
    assert!(graph.traverse_forward("A", 2).len() <= 1);
}

#[test]
fn evolution_log_append_only() {
    let sample = "## 出场记录日志\n| 章节 | 事件 |\n|------|------|\n| Ch1 | a |\n";
    let updated = append_evolution_log(sample, "出场记录日志", "| Ch2 | b |").unwrap();
    assert!(updated.contains("Ch1"));
    assert!(updated.contains("Ch2"));
}

#[tokio::test]
async fn fork_max_turns_boundary() {
    let tmp = TempDir::new().unwrap();
    std::fs::create_dir_all(tmp.path().join("skills")).unwrap();
    std::fs::create_dir_all(engine_config(&tmp).project_root.clone()).unwrap();
    let engine = AgentEngine::new(engine_config(&tmp)).unwrap();
    assert!(matches!(
        engine.fork(AgentType::KnowledgeAuditor, "  ".into()).await,
        Err(novel_core::AgentError::Fork(ForkError::EmptyTask))
    ));
}

#[test]
fn sqlite_rebuild_after_delete() {
    let tmp = TempDir::new().unwrap();
    let path = tmp.path().join("t.db");
    let db = Database::open(&path).unwrap();
    drop(db);
    std::fs::remove_file(&path).unwrap();
    Database::open(&path).unwrap();
}

#[test]
fn load_builtin_skills_from_repo() {
    let skills_dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("..")
        .join("skills");
    if skills_dir.exists() {
        let skills = load_skills_dir(&skills_dir).unwrap();
        assert!(!skills.is_empty());
        assert!(skills.iter().any(|s| s.id == "xianxia"));
    }
}
