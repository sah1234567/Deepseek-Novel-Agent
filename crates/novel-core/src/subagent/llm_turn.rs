//! Subagent LLM stream fetch (`subagent/runner::run_subagent_job` 抽出)。

use crate::engine::session_llm::{apply_session_usage, SessionLlmSnapshot};
use crate::fork_stream_subs::try_send_fork_overlay_event;
use crate::interrupt::finalize::{
    finalize_stream_cancel, FinalizeStreamCancelParams, ForkTranscriptSink,
};
use crate::message::assistant_from_completion;
use crate::subagent::helpers::fork_child_push;
use crate::subagent::helpers::{forward_subagent_stream_event, subagent_tool_context};
use crate::subagent::overflow::{
    build_partial_report, task_preview_120, OVERFLOW_KIND_INPUT_REJECTED,
    OVERFLOW_KIND_OUTPUT_TRUNCATED,
};
use crate::turn::StreamingToolDispatch;
use crate::{AgentError, AgentType, ChatMessage, Event};
use novel_deepseek::{
    is_context_length_exceeded, is_output_truncated, ChatClient, ChatRequestOptions,
    LlmChatMessage, LlmCompletion, LlmToolCall, StreamEvent, StreamOutcome,
};
use std::sync::{Arc, Mutex};

pub(crate) enum SubagentLlmFetch {
    Completion {
        completion: LlmCompletion,
        fork_dispatch: Option<Arc<Mutex<StreamingToolDispatch>>>,
    },
    LoopBreak,
    ReturnReport(String),
}

#[allow(clippy::too_many_arguments)]
pub(crate) async fn fetch_subagent_llm_completion(
    llm: &mut Option<ChatClient>,
    shared: &crate::EngineShared,
    agent_type: AgentType,
    task: &str,
    fork_run_id: &str,
    llm_msgs: &[LlmChatMessage],
    active_schemas: &[(String, String, serde_json::Value)],
    llm_snap: &SessionLlmSnapshot,
    event_tx: Option<&tokio::sync::mpsc::UnboundedSender<Event>>,
    child_messages: &mut Vec<ChatMessage>,
    message_snapshot: &[ChatMessage],
    inner_turn: u32,
) -> Result<SubagentLlmFetch, AgentError> {
    let fork_ctx = subagent_tool_context(shared, agent_type);
    if let Some(client) = llm {
        let cancel_flag = shared.abort_controller.cancel_flag();
        let (abort_tx, abort_rx) = novel_tools::abort_channel();
        let dispatch_arc = Arc::new(Mutex::new(StreamingToolDispatch::new(
            Arc::clone(&shared.registry),
            fork_ctx.clone(),
            1,
            abort_rx,
        )));
        let fork_dispatch = Some(Arc::clone(&dispatch_arc));
        let dispatch_cb = Arc::clone(&dispatch_arc);
        let registry_cb = Arc::clone(&shared.registry);
        let ctx_cb = fork_ctx.clone();
        let on_tool = move |tc: LlmToolCall| {
            if let Ok(mut d) = dispatch_cb.lock() {
                d.handle_ready(&registry_cb, &ctx_cb, None, tc, false);
            }
        };
        let _ = abort_tx;
        let fork_run_id_stream = fork_run_id.to_string();
        let event_tx_stream = event_tx.cloned();
        let subs_stream = Arc::clone(&shared.fork_stream_subs);
        let result = client
            .create_stream(
                llm_msgs,
                active_schemas,
                shared.settings.model.max_output_tokens,
                ChatRequestOptions::default(),
                move |ev: StreamEvent| {
                    if let Some(ref tx) = event_tx_stream {
                        forward_subagent_stream_event(&subs_stream, tx, &fork_run_id_stream, ev);
                    }
                },
                Some(on_tool),
                Some(cancel_flag),
            )
            .await;
        let completion = match result {
            Ok(StreamOutcome::Complete(c)) => c,
            Ok(StreamOutcome::Cancelled {
                partial,
                background_usage,
            }) => {
                if let Ok(mut d) = dispatch_arc.lock() {
                    d.discard();
                }
                tracing::debug!(
                    session_id = %shared.session.id,
                    %fork_run_id,
                    ?agent_type,
                    inner_turn,
                    message_count = child_messages.len(),
                    partial_tool_call_count = partial.tool_calls.len(),
                    "subagent_stream_cancelled"
                );
                let mut sink = ForkTranscriptSink {
                    db: &shared.session.db,
                    fork_run_id,
                    child: child_messages,
                };
                finalize_stream_cancel(FinalizeStreamCancelParams {
                    sink: &mut sink,
                    partial,
                    llm_messages: llm_msgs.to_vec(),
                    tool_schemas: active_schemas.to_vec(),
                    background_usage: Some(background_usage),
                    llm_snap: llm_snap.clone(),
                    shared: shared.clone(),
                    event_tx,
                    update_context_snapshot: false,
                })
                .await?;
                return Ok(SubagentLlmFetch::LoopBreak);
            }
            Err(e) if is_context_length_exceeded(&e) => {
                tracing::warn!(
                    session_id = %shared.session.id,
                    ?agent_type,
                    error = %e,
                    "subagent_context_length_exceeded"
                );
                if let Ok(mut d) = dispatch_arc.lock() {
                    d.discard();
                }
                child_messages.clear();
                child_messages.extend_from_slice(message_snapshot);
                shared.sub_agent_dec();
                return Ok(SubagentLlmFetch::ReturnReport(build_partial_report(
                    &agent_type.to_string(),
                    &task_preview_120(task),
                    OVERFLOW_KIND_INPUT_REJECTED,
                )));
            }
            Err(e) => {
                tracing::warn!(
                    session_id = %shared.session.id,
                    ?agent_type,
                    error = %e,
                    "subagent_llm_error"
                );
                if let Ok(mut d) = dispatch_arc.lock() {
                    d.discard();
                }
                return Ok(SubagentLlmFetch::LoopBreak);
            }
        };
        return Ok(SubagentLlmFetch::Completion {
            completion,
            fork_dispatch,
        });
    }
    Ok(SubagentLlmFetch::Completion {
        completion: ChatClient::offline_complete(llm_msgs),
        fork_dispatch: None,
    })
}

#[allow(clippy::too_many_arguments)]
pub(crate) fn subagent_after_completion(
    shared: &crate::EngineShared,
    agent_type: AgentType,
    task: &str,
    fork_run_id: &str,
    child_messages: &mut Vec<ChatMessage>,
    completion: &LlmCompletion,
    llm_snap: &SessionLlmSnapshot,
    event_tx: Option<&tokio::sync::mpsc::UnboundedSender<Event>>,
    inner_turn: u32,
) -> Result<Option<String>, AgentError> {
    if is_output_truncated(completion.stop_reason.as_deref()) {
        tracing::warn!(
            session_id = %shared.session.id,
            %fork_run_id,
            ?agent_type,
            inner_turn,
            tool_call_count = completion.tool_calls.len(),
            "subagent_output_truncated"
        );
        fork_child_push(
            &shared.session.db,
            fork_run_id,
            child_messages,
            assistant_from_completion(completion),
        )?;
        shared.sub_agent_dec();
        return Ok(Some(build_partial_report(
            &agent_type.to_string(),
            &task_preview_120(task),
            OVERFLOW_KIND_OUTPUT_TRUNCATED,
        )));
    }
    if let Some(u) = &completion.usage {
        apply_session_usage(shared, u, llm_snap, event_tx, false);
    }
    if let Some(tx) = event_tx {
        try_send_fork_overlay_event(
            &shared.fork_stream_subs,
            tx,
            Event::AssistantSegmentComplete {
                segment_index: inner_turn,
                fork_run_id: Some(fork_run_id.to_string()),
            },
        );
    }
    Ok(None)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::engine::session_llm::{build_chat_client, read_session_llm};
    use crate::test_env::StripDeepseekApiKey;
    use crate::EngineConfig;
    use novel_config::{save_agent_api_config, AgentApiConfig};
    use tempfile::TempDir;
    use wiremock::matchers::{method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    #[tokio::test(flavor = "current_thread")]
    async fn fetch_subagent_live_stream_via_wiremock() {
        let _offline = StripDeepseekApiKey::new();
        std::env::set_var("DEEPSEEK_API_KEY", "test-key");
        let server = MockServer::start().await;
        std::env::set_var("DEEPSEEK_API_BASE", server.uri());

        let sse = concat!(
            "data: {\"choices\":[{\"delta\":{\"content\":\"report\"},\"finish_reason\":null}],",
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
        let engine = crate::AgentEngine::new(EngineConfig {
            project_root: tmp.path().to_path_buf(),
            settings_path: tmp.path().join("settings.json"),
            db_path: tmp.path().join("state.db"),
            skills_dir: tmp.path().join("skills"),
            global_config_path: api_path,
        })
        .unwrap();
        let shared = engine.shared.clone();
        let snap = read_session_llm(&shared);
        let mut llm = build_chat_client(&snap, &shared.global_config_path);
        let fork_run_id = shared
            .session
            .db
            .create_fork_run(&shared.session.id, 1, "KnowledgeAuditor", "t", "test")
            .unwrap();
        let mut child_messages = vec![ChatMessage {
            role: "user".into(),
            content: "audit".into(),
            tool_call_id: None,
            tool_calls: None,
            reasoning_content: None,
            ..Default::default()
        }];
        let fetch = fetch_subagent_llm_completion(
            &mut llm,
            &shared,
            AgentType::KnowledgeAuditor,
            "audit",
            &fork_run_id,
            &crate::message::to_llm_messages(&child_messages),
            &[],
            &snap,
            None,
            &mut child_messages,
            &[],
            1,
        )
        .await
        .unwrap();
        assert!(matches!(fetch, SubagentLlmFetch::Completion { .. }));
        std::env::remove_var("DEEPSEEK_API_BASE");
    }
}
