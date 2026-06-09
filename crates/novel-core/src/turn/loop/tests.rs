use crate::message::tool_result_message;
use crate::subagent::drain_subagent_jobs;
use crate::turn::llm_stream::should_continue_inner_after_completion;
use crate::turn::StreamingToolDispatch;
use crate::turn::MSG_SEQ_USER;
use crate::{AgentEngine, AgentError, AgentType, ChatMessage, EngineConfig, Event};
use novel_deepseek::{LlmCompletion, LlmToolCall};
use novel_tools::{PendingSubagentWork, ToolCallSpec};
use std::sync::Arc;
use tempfile::TempDir;

fn test_config(tmp: &TempDir) -> EngineConfig {
    EngineConfig {
        project_root: tmp.path().to_path_buf(),
        settings_path: tmp.path().join("settings.json"),
        db_path: tmp.path().join("state.db"),
        skills_dir: tmp.path().join("skills"),
        global_config_path: tmp.path().join(".novel-agent/api_config.json"),
    }
}

#[tokio::test]
async fn offline_turn_produces_assistant_reply() {
    let _offline = crate::test_env::StripDeepseekApiKey::new();
    let tmp = TempDir::new().unwrap();
    std::fs::create_dir_all(tmp.path().join("skills")).unwrap();
    let mut engine = AgentEngine::new(test_config(&tmp)).unwrap();
    engine.handle_message("测试消息").await.unwrap();
    assert!(engine
        .messages
        .iter()
        .any(|m| m.role == "assistant" && m.content.contains("测试消息")));
}

#[tokio::test]
async fn messages_persisted_to_db() {
    let _offline = crate::test_env::StripDeepseekApiKey::new();
    let tmp = TempDir::new().unwrap();
    std::fs::create_dir_all(tmp.path().join("skills")).unwrap();
    let mut engine = AgentEngine::new(test_config(&tmp)).unwrap();
    engine.handle_message("持久化").await.unwrap();
    let stored = engine
        .shared
        .session
        .db
        .get_session_messages(&engine.shared.session.id, None)
        .unwrap();
    assert!(stored.len() >= 2);
}

#[tokio::test]
async fn default_hooks_do_not_enqueue_tasks() {
    use crate::hooks::{default_hook_config, knowledge_auditor_hook_task};
    let hooks = default_hook_config();
    let input = serde_json::json!({"file_path": "chapters/chapter-001.md"});
    assert!(knowledge_auditor_hook_task(&hooks, "Write", Some(&input), "written").is_none());
}

#[tokio::test]
async fn approve_unknown_tool_returns_validation() {
    let tmp = TempDir::new().unwrap();
    std::fs::create_dir_all(tmp.path().join("skills")).unwrap();
    let mut engine = AgentEngine::new(test_config(&tmp)).unwrap();
    let err = engine.approve_tool("missing-id", None).await.unwrap_err();
    assert!(matches!(err, AgentError::Validation(_)));
}

#[tokio::test]
async fn approve_pending_read_persists_tool_message() {
    let _offline = crate::test_env::StripDeepseekApiKey::new();
    let tmp = TempDir::new().unwrap();
    std::fs::create_dir_all(tmp.path().join("skills")).unwrap();
    std::fs::write(tmp.path().join("notes.md"), "approved body").unwrap();
    let mut engine = AgentEngine::new(test_config(&tmp)).unwrap();
    engine.pending_tools.insert(
        "t-approve".into(),
        ToolCallSpec {
            id: "t-approve".into(),
            name: "Read".into(),
            input: serde_json::json!({"file_path": "notes.md"}),
        },
    );
    engine.approve_tool("t-approve", None).await.unwrap();
    assert!(engine.pending_tools.is_empty());
    assert!(engine.messages.iter().any(|m| {
        m.role == "tool"
            && m.tool_call_id.as_deref() == Some("t-approve")
            && m.content.contains("approved body")
    }));
}

#[tokio::test]
async fn execute_stream_results_persists_tool_message() {
    let tmp = TempDir::new().unwrap();
    std::fs::create_dir_all(tmp.path().join("skills")).unwrap();
    let mut engine = AgentEngine::new(test_config(&tmp)).unwrap();
    let spec = ToolCallSpec {
        id: "t1".into(),
        name: "Read".into(),
        input: serde_json::json!({"file_path": "notes.md"}),
    };
    let pause = engine
        .execute_stream_results(
            vec![(
                "t1".into(),
                Ok(novel_tools::ToolOutput {
                    content: "file body".into(),
                    is_error: false,
                }),
            )],
            std::slice::from_ref(&spec),
            &["t1".into()],
            None,
            true,
        )
        .await
        .unwrap();
    assert!(!pause);
    assert!(engine
        .messages
        .iter()
        .any(|m| m.role == "tool" && m.tool_call_id.as_deref() == Some("t1")));
}

#[tokio::test]
async fn execute_stream_results_pauses_on_needs_user_input() {
    let tmp = TempDir::new().unwrap();
    std::fs::create_dir_all(tmp.path().join("skills")).unwrap();
    let mut engine = AgentEngine::new(test_config(&tmp)).unwrap();
    let spec = ToolCallSpec {
        id: "q1".into(),
        name: "AskUserQuestion".into(),
        input: serde_json::json!({}),
    };
    let pause = engine
        .execute_stream_results(
            vec![(
                "q1".into(),
                Err(novel_tools::ToolError::NeedsUserInput {
                    payload: novel_tools::AskUserQuestionPayload { questions: vec![] },
                }),
            )],
            std::slice::from_ref(&spec),
            &["q1".into()],
            None,
            false,
        )
        .await
        .unwrap();
    assert!(pause);
    assert_eq!(engine.pending_user_question.as_deref(), Some("q1"));
}

#[tokio::test]
async fn handle_ready_allows_read_tool() {
    use novel_tools::{abort_channel, default_registry, PermissionMode, ToolContext};
    let tmp = TempDir::new().unwrap();
    std::fs::write(tmp.path().join("x.md"), "body").unwrap();
    let registry = Arc::new(default_registry());
    let ctx = ToolContext {
        permission_mode: PermissionMode::Auto,
        project_root: tmp.path().to_path_buf(),
        ..ToolContext::new(tmp.path().to_path_buf())
    };
    let (_, abort_rx) = abort_channel();
    let mut dispatch = StreamingToolDispatch::new(registry.clone(), ctx.clone(), 4, abort_rx);
    dispatch.handle_ready(
        &registry,
        &ctx,
        None,
        LlmToolCall {
            id: "tc-read".into(),
            name: "Read".into(),
            arguments: r#"{"file_path":"x.md"}"#.into(),
        },
        true,
    );
    assert!(dispatch.handled_ids.contains("tc-read"));
    assert_eq!(dispatch.executed_specs.len(), 1);
}

#[tokio::test]
async fn answer_question_clears_pending_and_continues() {
    let _offline = crate::test_env::StripDeepseekApiKey::new();
    let tmp = TempDir::new().unwrap();
    std::fs::create_dir_all(tmp.path().join("skills")).unwrap();
    let mut engine = AgentEngine::new(test_config(&tmp)).unwrap();
    engine.pending_user_question = Some("q1".into());
    engine
        .messages
        .push(tool_result_message("q1", "等待用户回答问题后再继续。"));
    engine
        .answer_question("q1", serde_json::json!({"selections": {}}), None)
        .await
        .unwrap();
    assert!(engine.pending_user_question.is_none());
}

#[tokio::test]
async fn deny_tool_does_not_continue_while_question_pending() {
    let _offline = crate::test_env::StripDeepseekApiKey::new();
    let tmp = TempDir::new().unwrap();
    std::fs::create_dir_all(tmp.path().join("skills")).unwrap();
    let mut engine = AgentEngine::new(test_config(&tmp)).unwrap();
    engine.pending_user_question = Some("ask-q1".into());
    engine.pending_tools.insert(
        "write-1".into(),
        ToolCallSpec {
            id: "write-1".into(),
            name: "Write".into(),
            input: serde_json::json!({"file_path": "notes.md", "content": "x"}),
        },
    );
    let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();
    engine.deny_tool("write-1", None, Some(&tx)).await.unwrap();
    assert!(engine.pending_user_question.is_some());
    let mut saw_turn_complete = false;
    while let Ok(ev) = rx.try_recv() {
        if matches!(ev, Event::TurnComplete { .. }) {
            saw_turn_complete = true;
        }
    }
    assert!(!saw_turn_complete);
}

#[test]
fn inject_sub_agent_report_allocates_sequence_after_user_message() {
    let tmp = TempDir::new().unwrap();
    std::fs::create_dir_all(tmp.path().join("skills")).unwrap();
    let mut engine = AgentEngine::new(test_config(&tmp)).unwrap();
    engine.turn_number = 1;
    engine.turn_message_seq = 0;

    let user_msg = ChatMessage {
        role: "user".into(),
        content: "run consistency check".into(),
        tool_call_id: None,
        tool_calls: None,
        reasoning_content: None,
    };
    engine
        .persist_message_at_seq(&user_msg, MSG_SEQ_USER, None)
        .unwrap();
    engine.messages.push(user_msg);

    let assistant = ChatMessage {
        role: "assistant".into(),
        content: "forking sub-agent".into(),
        tool_call_id: None,
        tool_calls: None,
        reasoning_content: None,
    };
    engine.persist_message_alloc(&assistant).unwrap();
    engine.messages.push(assistant);

    engine
        .inject_sub_agent_report(AgentType::KnowledgeAuditor, "POV ok", None)
        .unwrap();

    let stored = engine
        .shared
        .session
        .db
        .get_session_messages(&engine.shared.session.id, None)
        .unwrap();
    let turn_one: Vec<_> = stored.iter().filter(|m| m.turn_number == 1).collect();
    assert_eq!(turn_one.len(), 3);
    assert_eq!(turn_one[0].sequence, MSG_SEQ_USER);
    assert_eq!(turn_one[1].sequence, 1);
    assert_eq!(turn_one[2].sequence, 2);
    assert!(turn_one[2]
        .content_json
        .to_string()
        .contains("子 Agent 完成"));
}

#[test]
fn build_message_rows_keeps_sub_agent_report_in_same_turn() {
    let tmp = TempDir::new().unwrap();
    std::fs::create_dir_all(tmp.path().join("skills")).unwrap();
    let mut engine = AgentEngine::new(test_config(&tmp)).unwrap();
    engine.turn_number = 1;
    engine.messages.clear();
    engine.messages.push(ChatMessage {
        role: "system".into(),
        content: "sys".into(),
        tool_call_id: None,
        tool_calls: None,
        reasoning_content: None,
    });
    engine.messages.push(ChatMessage {
        role: "user".into(),
        content: "hello".into(),
        tool_call_id: None,
        tool_calls: None,
        reasoning_content: None,
    });
    engine.messages.push(ChatMessage {
        role: "assistant".into(),
        content: "working".into(),
        tool_call_id: None,
        tool_calls: None,
        reasoning_content: None,
    });
    engine.messages.push(ChatMessage {
        role: "user".into(),
        content: format!(
            "{} KnowledgeAuditor]\nreport",
            AgentEngine::SUB_AGENT_REPORT_PREFIX
        ),
        tool_call_id: None,
        tool_calls: None,
        reasoning_content: None,
    });

    let rows = engine.build_message_rows();
    assert_eq!(rows.len(), 4);
    assert_eq!(rows[0].0, 0);
    assert_eq!(rows[0].1, 0);
    assert_eq!(rows[1].0, 1);
    assert_eq!(rows[1].1, MSG_SEQ_USER);
    assert_eq!(rows[2].0, 1);
    assert_eq!(rows[2].1, 1);
    assert_eq!(rows[3].0, 1);
    assert_eq!(rows[3].1, 2);
}

#[tokio::test]
async fn drain_subagent_jobs_injects_report_with_unique_sequences() {
    let _offline = crate::test_env::StripDeepseekApiKey::new();
    let tmp = TempDir::new().unwrap();
    std::fs::create_dir_all(tmp.path().join("skills")).unwrap();
    let mut engine = AgentEngine::new(test_config(&tmp)).unwrap();
    engine.turn_number = 1;
    engine.turn_message_seq = 0;

    let user_msg = ChatMessage {
        role: "user".into(),
        content: "fork consistency check".into(),
        tool_call_id: None,
        tool_calls: None,
        reasoning_content: None,
    };
    engine
        .persist_message_at_seq(&user_msg, MSG_SEQ_USER, None)
        .unwrap();
    engine.messages.push(user_msg);

    let tool_msg = tool_result_message("tc-fork", "Subagent 已启动");
    engine.persist_message_alloc(&tool_msg).unwrap();
    engine.messages.push(tool_msg);

    {
        let mut guard = engine
            .shared
            .subagent_queue
            .lock()
            .expect("subagent queue lock");
        guard.push(PendingSubagentWork {
            agent_type: "KnowledgeAuditor".into(),
            task: "审计 chapters/chapter-001.md".into(),
            parent_tool_call_id: Some("tc-fork".into()),
        });
    }

    drain_subagent_jobs(&mut engine, None).await.unwrap();

    let stored = engine
        .shared
        .session
        .db
        .get_session_messages(&engine.shared.session.id, None)
        .unwrap();
    let turn_one: Vec<_> = stored.iter().filter(|m| m.turn_number == 1).collect();
    assert!(
        turn_one
            .iter()
            .any(|m| m.content_json.to_string().contains("子 Agent 完成")),
        "expected sub-agent report in DB after sync drain"
    );
    let mut seen = std::collections::HashSet::new();
    for m in &turn_one {
        assert!(
            seen.insert((m.turn_number, m.sequence)),
            "duplicate (turn, sequence)=({}, {})",
            m.turn_number,
            m.sequence
        );
    }
    assert_eq!(
        engine
            .shared
            .sub_agent_count
            .load(std::sync::atomic::Ordering::SeqCst),
        0
    );
}

fn fork_tool_result_texts(fork_msgs: &[novel_state::ForkMessage]) -> Vec<String> {
    fork_msgs
        .iter()
        .filter(|fm| fm.role == "tool")
        .filter_map(|fm| fm.content_json.get("content").and_then(|c| c.as_str()))
        .filter(|text| text.len() > 24 && !text.starts_with("Error:"))
        .map(str::to_string)
        .collect()
}

fn assert_fork_bodies_absent_from_parent(parent_llm: &str, fork_msgs: &[novel_state::ForkMessage]) {
    for text in fork_tool_result_texts(fork_msgs) {
        assert!(
            !parent_llm.contains(&text),
            "fork tool result leaked into parent LLM context: {text}"
        );
    }
}

#[tokio::test]
async fn parent_llm_context_excludes_fork_transcript() {
    let _offline = crate::test_env::StripDeepseekApiKey::new();
    let tmp = TempDir::new().unwrap();
    std::fs::create_dir_all(tmp.path().join("skills")).unwrap();
    let mut engine = AgentEngine::new(test_config(&tmp)).unwrap();
    engine.turn_number = 1;
    engine.turn_message_seq = 0;

    let user_msg = ChatMessage {
        role: "user".into(),
        content: "fork isolation check".into(),
        tool_call_id: None,
        tool_calls: None,
        reasoning_content: None,
    };
    engine
        .persist_message_at_seq(&user_msg, MSG_SEQ_USER, None)
        .unwrap();
    engine.messages.push(user_msg);

    let tool_msg = tool_result_message("tc-fork-iso", "Subagent 已启动");
    engine.persist_message_alloc(&tool_msg).unwrap();
    engine.messages.push(tool_msg);

    {
        let mut guard = engine
            .shared
            .subagent_queue
            .lock()
            .expect("subagent queue lock");
        guard.push(PendingSubagentWork {
            agent_type: "KnowledgeAuditor".into(),
            task: "审计 chapters/chapter-001.md".into(),
            parent_tool_call_id: Some("tc-fork-iso".into()),
        });
    }

    drain_subagent_jobs(&mut engine, None).await.unwrap();

    let stored = engine
        .shared
        .session
        .db
        .get_session_messages(&engine.shared.session.id, None)
        .unwrap();
    let report = stored
        .iter()
        .filter(|m| m.role != "system")
        .find(|m| m.content_json.to_string().contains("子 Agent 完成"))
        .expect("sub-agent report in parent session");
    let fork_run_id = report
        .content_json
        .get("fork_run_id")
        .and_then(|v| v.as_str())
        .expect("fork_run_id metadata on report");

    let fork_msgs = engine
        .shared
        .session
        .db
        .get_fork_messages(fork_run_id)
        .unwrap();
    assert!(
        !fork_msgs.is_empty(),
        "fork transcript should be persisted separately"
    );

    let parent_llm: String = crate::message::to_llm_messages(&engine.messages)
        .iter()
        .map(|m| format!("{:?}", m))
        .collect();
    assert!(
        parent_llm.contains("子 Agent 完成"),
        "tool-path report summary must remain in parent LLM context"
    );

    assert_fork_bodies_absent_from_parent(&parent_llm, &fork_msgs);
}

#[tokio::test]
async fn drain_subagent_jobs_injects_multiple_reports_in_order() {
    let _offline = crate::test_env::StripDeepseekApiKey::new();
    let tmp = TempDir::new().unwrap();
    std::fs::create_dir_all(tmp.path().join("skills")).unwrap();
    let mut engine = AgentEngine::new(test_config(&tmp)).unwrap();
    engine.turn_number = 1;
    engine.turn_message_seq = 0;

    let user_msg = ChatMessage {
        role: "user".into(),
        content: "parallel fork".into(),
        tool_call_id: None,
        tool_calls: None,
        reasoning_content: None,
    };
    engine
        .persist_message_at_seq(&user_msg, MSG_SEQ_USER, None)
        .unwrap();
    engine.messages.push(user_msg);
    let tool_msg = tool_result_message("tc-fork-batch", "batch started");
    engine.persist_message_alloc(&tool_msg).unwrap();
    engine.messages.push(tool_msg);

    {
        let mut guard = engine
            .shared
            .subagent_queue
            .lock()
            .expect("subagent queue lock");
        guard.push(PendingSubagentWork {
            agent_type: "KnowledgeAuditor".into(),
            task: "任务 A：chapter-001".into(),
            parent_tool_call_id: Some("tc-fork-a".into()),
        });
        guard.push(PendingSubagentWork {
            agent_type: "ChapterCraftAnalyzer".into(),
            task: "任务 B：chapter-001".into(),
            parent_tool_call_id: Some("tc-fork-b".into()),
        });
    }

    drain_subagent_jobs(&mut engine, None).await.unwrap();

    let stored = engine
        .shared
        .session
        .db
        .get_session_messages(&engine.shared.session.id, None)
        .unwrap();
    let reports: Vec<_> = stored
        .iter()
        .filter(|m| m.role != "system" && m.content_json.to_string().contains("子 Agent 完成"))
        .collect();
    assert_eq!(reports.len(), 2);
    assert_eq!(reports[0].sequence, 2);
    assert_eq!(reports[1].sequence, 3);
    assert!(reports[0]
        .content_json
        .to_string()
        .contains("KnowledgeAuditor"));
    assert!(reports[1]
        .content_json
        .to_string()
        .contains("ChapterCraftAnalyzer"));
}

#[test]
fn resume_inner_turn_counts_existing_assistants() {
    let tmp = TempDir::new().unwrap();
    let engine = AgentEngine::new(test_config(&tmp)).unwrap();
    assert_eq!(engine.resume_inner_turn_from_messages(), 0);

    let mut engine = engine;
    engine.messages.push(ChatMessage {
        role: "assistant".into(),
        content: "hi".into(),
        tool_call_id: None,
        tool_calls: Some(vec![crate::ToolCallRecord {
            id: "tc1".into(),
            name: "AskUserQuestion".into(),
            arguments: serde_json::json!({}),
        }]),
        reasoning_content: None,
    });
    assert_eq!(engine.resume_inner_turn_from_messages(), 1);
}

#[test]
fn should_continue_inner_after_reasoning_only_completion() {
    let c = LlmCompletion {
        content: None,
        reasoning_content: Some("plan WebSearch next".into()),
        tool_calls: vec![],
        usage: None,
        stop_reason: Some("stop".into()),
    };
    assert!(should_continue_inner_after_completion(&c));

    let with_tools = LlmCompletion {
        tool_calls: vec![LlmToolCall {
            id: "t1".into(),
            name: "WebSearch".into(),
            arguments: "{}".into(),
        }],
        ..Default::default()
    };
    assert!(!should_continue_inner_after_completion(&with_tools));

    let with_text = LlmCompletion {
        content: Some("done".into()),
        reasoning_content: Some("thought".into()),
        ..Default::default()
    };
    assert!(!should_continue_inner_after_completion(&with_text));
}

#[tokio::test]
async fn compact_and_sync_clears_read_file_cache() {
    use novel_tools::{ReadCacheEntry, ReadCacheSource};
    use std::path::PathBuf;

    let tmp = TempDir::new().unwrap();
    std::fs::create_dir_all(tmp.path().join("skills")).unwrap();
    let mut engine = AgentEngine::new(test_config(&tmp)).unwrap();
    engine.messages.push(ChatMessage {
        role: "user".into(),
        content: "chapter work".into(),
        tool_call_id: None,
        tool_calls: None,
        reasoning_content: None,
    });
    engine.messages.push(ChatMessage {
        role: "assistant".into(),
        content: "ok".into(),
        tool_call_id: None,
        tool_calls: None,
        reasoning_content: None,
    });
    engine.last_context_tokens = 850_000;

    engine.shared.read_file_cache.insert(
        PathBuf::from("chapters/ch01.md"),
        ReadCacheEntry {
            mtime_secs: 1,
            raw_content: "line".into(),
            offset: None,
            limit: None,
            total_lines: 1,
            source: ReadCacheSource::Read,
            transcript_committed: false,
            committed_spans: Vec::new(),
            committed_offset: None,
            committed_limit: None,
        },
    );
    assert_eq!(engine.shared.read_file_cache.len(), 1);

    engine.compact_and_sync(None).await.unwrap();
    assert!(engine.shared.read_file_cache.is_empty());
}

#[tokio::test]
async fn compact_and_sync_skipped_when_under_threshold_keeps_read_cache() {
    use novel_tools::{ReadCacheEntry, ReadCacheSource};
    use std::path::PathBuf;

    let tmp = TempDir::new().unwrap();
    std::fs::create_dir_all(tmp.path().join("skills")).unwrap();
    let mut engine = AgentEngine::new(test_config(&tmp)).unwrap();
    engine.last_context_tokens = 0;

    engine.shared.read_file_cache.insert(
        PathBuf::from("chapters/ch01.md"),
        ReadCacheEntry {
            mtime_secs: 1,
            raw_content: "line".into(),
            offset: None,
            limit: None,
            total_lines: 1,
            source: ReadCacheSource::Read,
            transcript_committed: false,
            committed_spans: Vec::new(),
            committed_offset: None,
            committed_limit: None,
        },
    );

    engine.compact_and_sync(None).await.unwrap();
    assert_eq!(engine.shared.read_file_cache.len(), 1);
}

mod compaction_tokens {
    use super::test_config;
    use crate::engine::session_llm::{apply_session_usage, read_session_llm};
    use crate::{AgentEngine, ChatMessage, EngineConfig, Event};
    use novel_config::{save_agent_api_config, AgentApiConfig};
    use novel_deepseek::{LlmCompletion, TokenUsage};
    use tempfile::TempDir;
    use tokio::sync::mpsc;
    use wiremock::matchers::{method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    fn test_config_with_api(tmp: &TempDir, api_config_path: std::path::PathBuf) -> EngineConfig {
        EngineConfig {
            global_config_path: api_config_path,
            ..test_config(tmp)
        }
    }

    fn push_user_assistant_pairs(engine: &mut AgentEngine, pairs: usize) {
        for i in 0..pairs {
            engine.messages.push(ChatMessage {
                role: "user".into(),
                content: format!("user turn {i}"),
                tool_call_id: None,
                tool_calls: None,
                reasoning_content: None,
            });
            engine.messages.push(ChatMessage {
                role: "assistant".into(),
                content: format!("assistant reply {i}"),
                tool_call_id: None,
                tool_calls: None,
                reasoning_content: None,
            });
        }
    }

    #[tokio::test(flavor = "current_thread")]
    async fn compaction_summary_api_bills_without_context_snapshot_change() {
        let _offline = crate::test_env::StripDeepseekApiKey::new();
        std::env::set_var("DEEPSEEK_API_KEY", "test-key");
        let server = MockServer::start().await;
        std::env::set_var("DEEPSEEK_API_BASE", server.uri());

        let sse = concat!(
            "data: {\"choices\":[{\"delta\":{\"content\":\"session summary\"},\"finish_reason\":null}],",
            "\"usage\":{\"prompt_tokens\":100,\"completion_tokens\":10,",
            "\"prompt_tokens_details\":{\"cached_tokens\":30}}}\n\n",
            "data: [DONE]\n\n"
        );
        Mock::given(method("POST"))
            .and(path("/chat/completions"))
            .respond_with(
                ResponseTemplate::new(200)
                    .insert_header("content-type", "text/event-stream")
                    .set_body_string(sse),
            )
            .mount(&server)
            .await;

        let tmp = TempDir::new().unwrap();
        std::fs::create_dir_all(tmp.path().join("skills")).unwrap();
        let api_path = tmp.path().join(".novel-agent/api_config.json");
        std::fs::create_dir_all(api_path.parent().unwrap()).unwrap();
        save_agent_api_config(
            &api_path,
            &AgentApiConfig {
                api_key: "test-key".into(),
                api_base: server.uri(),
            },
        )
        .unwrap();

        let mut engine = AgentEngine::new(test_config_with_api(&tmp, api_path)).unwrap();
        push_user_assistant_pairs(&mut engine, 6);
        engine.last_context_tokens = 850_000;
        engine.init_llm();
        assert!(engine.llm.is_some());

        let (tx, mut rx) = mpsc::unbounded_channel();
        let snap = read_session_llm(&engine.shared);
        let session_id = engine.shared.session.id.clone();
        apply_session_usage(
            &engine.shared,
            &TokenUsage {
                cache_hit_tokens: 1,
                cache_miss_tokens: 2,
                completion_tokens: 3,
                reasoning_tokens: 0,
            },
            &snap,
            Some(&tx),
            true,
        );
        let _ = rx.try_recv().expect("pre-compaction emit");
        let before = engine
            .shared
            .session
            .db
            .get_session(&session_id)
            .unwrap()
            .unwrap();
        assert_eq!(before.context_tokens, 6);

        engine.compact_and_sync(Some(&tx)).await.unwrap();

        let after = engine
            .shared
            .session
            .db
            .get_session(&session_id)
            .unwrap()
            .unwrap();
        assert_eq!(after.cache_hit_tokens, 1 + 30);
        assert_eq!(after.cache_miss_tokens, 2 + 70);
        assert_eq!(after.completion_tokens, 3 + 10);
        assert_eq!(after.context_tokens, 6);

        let summary_emit = std::iter::from_fn(|| rx.try_recv().ok()).find(|evt| {
            matches!(
                evt,
                Event::SessionTokensUpdated {
                    cache_hit_tokens: 31,
                    cache_miss_tokens: 72,
                    completion_tokens: 13,
                    context_tokens: 6,
                }
            )
        });
        assert!(
            summary_emit.is_some(),
            "expected SessionTokensUpdated after summary API billing"
        );
        assert_eq!(engine.last_context_tokens, 0);

        std::env::remove_var("DEEPSEEK_API_BASE");
    }

    #[tokio::test]
    async fn post_compaction_main_api_updates_context_and_emits() {
        let tmp = TempDir::new().unwrap();
        std::fs::create_dir_all(tmp.path().join("skills")).unwrap();
        let mut engine = AgentEngine::new(test_config(&tmp)).unwrap();
        let (tx, mut rx) = mpsc::unbounded_channel();

        let snap = read_session_llm(&engine.shared);
        apply_session_usage(
            &engine.shared,
            &TokenUsage {
                cache_hit_tokens: 100,
                cache_miss_tokens: 200,
                completion_tokens: 50,
                reasoning_tokens: 0,
            },
            &snap,
            Some(&tx),
            true,
        );
        let _ = rx.try_recv().expect("pre-compaction emit");

        engine.last_context_tokens = 0;

        let completion = LlmCompletion {
            content: Some("after compaction".into()),
            usage: Some(TokenUsage {
                cache_hit_tokens: 10,
                cache_miss_tokens: 20,
                completion_tokens: 5,
                reasoning_tokens: 0,
            }),
            ..Default::default()
        };
        engine.record_usage(&completion, Some(&tx));

        let session = engine
            .shared
            .session
            .db
            .get_session(&engine.shared.session.id)
            .unwrap()
            .unwrap();
        assert_eq!(session.context_tokens, 35);
        assert_eq!(
            engine.last_turn_usage.as_ref().unwrap().cache_hit_tokens,
            10
        );

        let evt = rx.try_recv().expect("post-compaction main api emit");
        assert!(matches!(
            evt,
            Event::SessionTokensUpdated {
                cache_hit_tokens: 110,
                cache_miss_tokens: 220,
                completion_tokens: 55,
                context_tokens: 35,
            }
        ));
    }
}

mod llm_stream {
    use super::test_config;
    use crate::turn::llm_stream::forward_main_stream_event;
    use crate::{AgentEngine, ContentBlockKind, EngineConfig};
    use novel_config::{save_agent_api_config, AgentApiConfig};
    use novel_deepseek::StreamEvent;
    use tempfile::TempDir;
    use tokio::sync::mpsc;
    use wiremock::matchers::{method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    fn test_config_with_api(tmp: &TempDir, api_config_path: std::path::PathBuf) -> EngineConfig {
        EngineConfig {
            global_config_path: api_config_path,
            ..test_config(tmp)
        }
    }

    #[test]
    fn forward_main_stream_event_emits_ui_events() {
        let (tx, mut rx) = mpsc::unbounded_channel();
        forward_main_stream_event(
            &tx,
            None,
            StreamEvent::ContentBlockDelta {
                index: 0,
                delta: "x".into(),
                kind: ContentBlockKind::Text,
            },
        );
        forward_main_stream_event(
            &tx,
            None,
            StreamEvent::ToolUseStarted {
                index: 0,
                tool_call_id: "tc".into(),
                name: "Read".into(),
            },
        );
        forward_main_stream_event(
            &tx,
            None,
            StreamEvent::StreamError {
                message: "boom".into(),
                retryable: true,
            },
        );
        assert!(rx.try_recv().is_ok());
    }

    #[tokio::test(flavor = "current_thread")]
    async fn streaming_turn_hits_call_llm_via_wiremock() {
        let _offline = crate::test_env::StripDeepseekApiKey::new();
        std::env::set_var("DEEPSEEK_API_KEY", "test-key");
        let server = MockServer::start().await;
        std::env::set_var("DEEPSEEK_API_BASE", server.uri());

        let sse = concat!(
            "data: {\"choices\":[{\"delta\":{\"content\":\"hello\"},\"finish_reason\":null}],",
            "\"usage\":{\"prompt_tokens\":1,\"completion_tokens\":1}}\n\n",
            "data: [DONE]\n\n"
        );
        Mock::given(method("POST"))
            .and(path("/chat/completions"))
            .respond_with(
                ResponseTemplate::new(200)
                    .insert_header("content-type", "text/event-stream")
                    .set_body_string(sse),
            )
            .mount(&server)
            .await;

        let tmp = TempDir::new().unwrap();
        std::fs::create_dir_all(tmp.path().join("skills")).unwrap();
        let api_path = tmp.path().join(".novel-agent/api_config.json");
        std::fs::create_dir_all(api_path.parent().unwrap()).unwrap();
        save_agent_api_config(
            &api_path,
            &AgentApiConfig {
                api_key: "test-key".into(),
                api_base: server.uri(),
            },
        )
        .unwrap();
        let mut engine = AgentEngine::new(test_config_with_api(&tmp, api_path)).unwrap();
        engine.handle_message("stream ping").await.unwrap();
        assert!(engine
            .messages
            .iter()
            .any(|m| m.role == "assistant" && m.content.contains("hello")));
        std::env::remove_var("DEEPSEEK_API_BASE");
    }

    #[tokio::test]
    async fn streaming_turn_with_tool_calls_hits_finish_batch() {
        let _offline = crate::test_env::StripDeepseekApiKey::new();
        std::env::set_var("DEEPSEEK_API_KEY", "test-key");
        let server = MockServer::start().await;
        std::env::set_var("DEEPSEEK_API_BASE", server.uri());

        // Inner ReAct loop calls the LLM again after tool results; a single tool_calls
        // mock would loop forever (max_react_loops defaults to 0 = unlimited).
        let tool_sse = concat!(
            "data: {\"choices\":[{\"delta\":{\"tool_calls\":[{\"index\":0,\"id\":\"tc-r\",\"function\":",
            "{\"name\":\"Read\",\"arguments\":\"{\\\"file_path\\\":\\\"wm.txt\\\"}\"}}]},\"finish_reason\":null}]}\n\n",
            "data: {\"choices\":[{\"delta\":{},\"finish_reason\":\"tool_calls\"}],",
            "\"usage\":{\"prompt_tokens\":1,\"completion_tokens\":1}}\n\n",
            "data: [DONE]\n\n"
        );
        let stop_sse = concat!(
            "data: {\"choices\":[{\"delta\":{\"content\":\"done\"},\"finish_reason\":null}],",
            "\"usage\":{\"prompt_tokens\":1,\"completion_tokens\":1}}\n\n",
            "data: [DONE]\n\n"
        );
        Mock::given(method("POST"))
            .and(path("/chat/completions"))
            .respond_with(
                ResponseTemplate::new(200)
                    .insert_header("content-type", "text/event-stream")
                    .set_body_string(tool_sse),
            )
            .up_to_n_times(1)
            .mount(&server)
            .await;
        Mock::given(method("POST"))
            .and(path("/chat/completions"))
            .respond_with(
                ResponseTemplate::new(200)
                    .insert_header("content-type", "text/event-stream")
                    .set_body_string(stop_sse),
            )
            .mount(&server)
            .await;

        let tmp = TempDir::new().unwrap();
        std::fs::write(tmp.path().join("wm.txt"), "tool body").unwrap();
        std::fs::create_dir_all(tmp.path().join("skills")).unwrap();
        let api_path = tmp.path().join(".novel-agent/api_config.json");
        std::fs::create_dir_all(api_path.parent().unwrap()).unwrap();
        save_agent_api_config(
            &api_path,
            &AgentApiConfig {
                api_key: "test-key".into(),
                api_base: server.uri(),
            },
        )
        .unwrap();
        let mut engine = AgentEngine::new(test_config_with_api(&tmp, api_path)).unwrap();
        engine.handle_message("read wm").await.unwrap();
        assert!(engine
            .messages
            .iter()
            .any(|m| m.role == "tool" && m.content.contains("tool body")));
        std::env::remove_var("DEEPSEEK_API_BASE");
    }
}
