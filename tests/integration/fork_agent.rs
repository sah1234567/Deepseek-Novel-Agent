#![allow(clippy::unwrap_used)]
#![allow(clippy::expect_used)]

use novel_core::{AgentEngine, AgentType, EngineConfig};
use tempfile::TempDir;

#[tokio::test]
async fn fork_shares_system_prompt() {
    let tmp = TempDir::new().unwrap();
    std::fs::create_dir_all(tmp.path().join("novels/demo")).unwrap();
    let cfg = EngineConfig {
        project_root: tmp.path().join("novels/demo"),
        settings_path: tmp.path().join("settings.json"),
        db_path: tmp.path().join("state.db"),
        skills_dir: tmp.path().join("skills"),
        global_config_path: tmp.path().join(".novel-agent/api_config.json"),
    };
    std::fs::create_dir_all(&cfg.skills_dir).unwrap();
    let mut engine = AgentEngine::new(cfg).unwrap();
    engine.handle_message("策划世界观").await.unwrap();
    let child = engine
        .fork(
            AgentType::KnowledgeAuditor,
            "审计第1章：chapters/chapter-001.md".into(),
        )
        .await
        .unwrap();
    // 子 agent 消息 = system prompt + task_message（仅 2 条）
    assert_eq!(child.messages.len(), 2);
    assert!(child.messages[0].role == "system");
    assert!(child.messages[1].content.contains("审计第1章"));
}

#[tokio::test]
async fn fork_from_system_only_deepseek_cache() {
    let tmp = TempDir::new().unwrap();
    std::fs::create_dir_all(tmp.path().join("novels/demo")).unwrap();
    let cfg = EngineConfig {
        project_root: tmp.path().join("novels/demo"),
        settings_path: tmp.path().join("settings.json"),
        db_path: tmp.path().join("state.db"),
        skills_dir: tmp.path().join("skills"),
        global_config_path: tmp.path().join(".novel-agent/api_config.json"),
    };
    std::fs::create_dir_all(&cfg.skills_dir).unwrap();
    let mut engine = AgentEngine::new(cfg).unwrap();
    for i in 0..5 {
        engine
            .handle_message(&format!("历史消息 {i}"))
            .await
            .unwrap();
    }
    let child = engine
        .fork(
            AgentType::KnowledgeAuditor,
            "扫描 chapter-001 知识库遗漏".into(),
        )
        .await
        .unwrap();
    // Sub-agent prompt = shared-base (+ audit skill body), not the parent's orchestrator role.
    // Within the child, system is a fixed prefix so DeepSeek KV cache can hit across its turns.
    assert_eq!(child.messages.len(), 2);
    assert_eq!(child.messages[0].role, "system");
    assert!(child.messages[0]
        .content
        .contains("\u{5171}\u{4eab}\u{5e95}\u{5ea7}")); // 共享底座
    assert!(!child.messages[0]
        .content
        .contains("\u{56fe}\u{7f16}\u{6392}\u{5668}")); // 图编排器
    assert!(
        child.messages[0]
            .content
            .contains("\u{77e5}\u{8bc6}\u{5e93}") // 知识库
            || child.messages[0].content.contains("\u{5ba1}\u{8ba1}") // 审计
    );
}

#[tokio::test]
async fn nested_fork_rejected_when_sub_agent_running() {
    let tmp = TempDir::new().unwrap();
    std::fs::create_dir_all(tmp.path().join("novels/demo")).unwrap();
    let cfg = EngineConfig {
        project_root: tmp.path().join("novels/demo"),
        settings_path: tmp.path().join("settings.json"),
        db_path: tmp.path().join("state.db"),
        skills_dir: tmp.path().join("skills"),
        global_config_path: tmp.path().join(".novel-agent/api_config.json"),
    };
    std::fs::create_dir_all(&cfg.skills_dir).unwrap();
    let mut engine = AgentEngine::new(cfg).unwrap();
    engine.handle_message("hi").await.unwrap();

    engine
        .shared
        .sub_agent_count
        .store(1, std::sync::atomic::Ordering::SeqCst);
    let result = engine
        .fork(AgentType::KnowledgeAuditor, "写第2章".into())
        .await;
    assert!(result.is_err());
}

#[tokio::test]
async fn fork_general_purpose_includes_custom_task() {
    let tmp = TempDir::new().unwrap();
    std::fs::create_dir_all(tmp.path().join("novels/demo")).unwrap();
    let cfg = EngineConfig {
        project_root: tmp.path().join("novels/demo"),
        settings_path: tmp.path().join("settings.json"),
        db_path: tmp.path().join("state.db"),
        skills_dir: tmp.path().join("skills"),
        global_config_path: tmp.path().join(".novel-agent/api_config.json"),
    };
    std::fs::create_dir_all(&cfg.skills_dir).unwrap();
    let engine = AgentEngine::new(cfg).unwrap();
    let custom = "对比 chapter-003 与 chapter-005 细纲人物出场";
    let child = engine
        .fork(AgentType::GeneralPurpose, custom.into())
        .await
        .unwrap();
    assert_eq!(child.messages.len(), 2);
    assert!(child.messages[1].content.contains("## 自定义任务"));
    assert!(child.messages[1].content.contains(custom));
    assert!(!child.messages[1].content.contains("你是章节写作 Agent"));
}
