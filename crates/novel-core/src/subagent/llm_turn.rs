//! Subagent LLM stream fetch (`subagent/runner::run_subagent_job` 抽出)。

use crate::engine::session_llm::apply_session_usage;
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
use crate::subagent::params::{SubagentCompletionParams, SubagentLlmFetchParams};
use crate::turn::StreamingToolDispatch;
use crate::{AgentError, ChatMessage, Event};
use novel_deepseek::{
    is_context_length_exceeded, is_output_truncated, ChatClient, ChatRequestOptions,
    ChatStreamConfig, LlmChatMessage, LlmCompletion, LlmToolCall, StreamEvent, StreamOutcome,
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

fn subagent_offline_fetch(llm_msgs: &[LlmChatMessage]) -> SubagentLlmFetch {
    SubagentLlmFetch::Completion {
        completion: ChatClient::offline_complete(llm_msgs),
        fork_dispatch: None,
    }
}

async fn subagent_handle_context_overflow(
    dispatch_arc: &Arc<Mutex<StreamingToolDispatch>>,
    params: &SubagentLlmFetchParams<'_>,
    child_messages: &mut Vec<ChatMessage>,
) -> SubagentLlmFetch {
    if let Ok(mut d) = dispatch_arc.lock() {
        d.discard();
    }
    child_messages.clear();
    child_messages.extend_from_slice(params.message_snapshot);
    params.shared.sub_agent_dec();
    SubagentLlmFetch::ReturnReport(build_partial_report(
        &params.agent_type.to_string(),
        &task_preview_120(params.task),
        OVERFLOW_KIND_INPUT_REJECTED,
    ))
}

async fn subagent_handle_stream_cancel(
    dispatch_arc: &Arc<Mutex<StreamingToolDispatch>>,
    child_messages: &mut Vec<ChatMessage>,
    params: &SubagentLlmFetchParams<'_>,
    partial: LlmCompletion,
    background_usage: novel_deepseek::BackgroundUsageRx,
) -> Result<SubagentLlmFetch, AgentError> {
    if let Ok(mut d) = dispatch_arc.lock() {
        d.discard();
    }
    tracing::debug!(
        session_id = %params.shared.session.id,
        fork_run_id = %params.fork_run_id,
        ?params.agent_type,
        inner_turn = params.inner_turn,
        message_count = child_messages.len(),
        partial_tool_call_count = partial.tool_calls.len(),
        "subagent_stream_cancelled"
    );
    let mut sink = ForkTranscriptSink {
        db: &params.shared.session.db,
        fork_run_id: params.fork_run_id,
        child: child_messages,
    };
    finalize_stream_cancel(FinalizeStreamCancelParams {
        sink: &mut sink,
        partial,
        llm_messages: params.llm_msgs.to_vec(),
        tool_schemas: params.active_schemas.to_vec(),
        background_usage: Some(background_usage),
        llm_snap: params.llm_snap.clone(),
        shared: params.shared.clone(),
        event_tx: params.event_tx,
        update_context_snapshot: false,
    })
    .await?;
    Ok(SubagentLlmFetch::LoopBreak)
}

async fn subagent_run_live_stream(
    client: &mut ChatClient,
    child_messages: &mut Vec<ChatMessage>,
    params: &SubagentLlmFetchParams<'_>,
) -> Result<SubagentLlmFetch, AgentError> {
    let shared = params.shared;
    let fork_ctx = subagent_tool_context(shared, params.agent_type);
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
    let fork_run_id_stream = params.fork_run_id.to_string();
    let event_tx_stream = params.event_tx.cloned();
    let subs_stream = Arc::clone(&shared.fork_stream_subs);
    let result = client
        .create_stream(
            params.llm_msgs,
            params.active_schemas,
            ChatStreamConfig {
                max_tokens: shared.settings.model.max_output_tokens,
                options: ChatRequestOptions::default(),
                cancel: Some(cancel_flag),
            },
            move |ev: StreamEvent| {
                if let Some(ref tx) = event_tx_stream {
                    forward_subagent_stream_event(&subs_stream, tx, &fork_run_id_stream, ev);
                }
            },
            Some(on_tool),
        )
        .await;
    match result {
        Ok(StreamOutcome::Complete(completion)) => Ok(SubagentLlmFetch::Completion {
            completion,
            fork_dispatch,
        }),
        Ok(StreamOutcome::Cancelled {
            partial,
            background_usage,
        }) => {
            subagent_handle_stream_cancel(
                &dispatch_arc,
                child_messages,
                params,
                partial,
                background_usage,
            )
            .await
        }
        Err(e) if is_context_length_exceeded(&e) => {
            tracing::warn!(
                session_id = %shared.session.id,
                ?params.agent_type,
                error = %e,
                "subagent_context_length_exceeded"
            );
            Ok(subagent_handle_context_overflow(&dispatch_arc, params, child_messages).await)
        }
        Err(e) => {
            tracing::warn!(
                session_id = %shared.session.id,
                ?params.agent_type,
                error = %e,
                "subagent_llm_error"
            );
            if let Ok(mut d) = dispatch_arc.lock() {
                d.discard();
            }
            Ok(SubagentLlmFetch::LoopBreak)
        }
    }
}

pub(crate) async fn fetch_subagent_llm_completion(
    llm: &mut Option<ChatClient>,
    child_messages: &mut Vec<ChatMessage>,
    params: &SubagentLlmFetchParams<'_>,
) -> Result<SubagentLlmFetch, AgentError> {
    if let Some(client) = llm {
        return subagent_run_live_stream(client, child_messages, params).await;
    }
    Ok(subagent_offline_fetch(params.llm_msgs))
}

pub(crate) fn subagent_after_completion(
    child_messages: &mut Vec<ChatMessage>,
    completion: &LlmCompletion,
    params: &SubagentCompletionParams<'_>,
) -> Result<Option<String>, AgentError> {
    if is_output_truncated(completion.stop_reason.as_deref()) {
        tracing::warn!(
            session_id = %params.shared.session.id,
            fork_run_id = %params.fork_run_id,
            ?params.agent_type,
            inner_turn = params.inner_turn,
            tool_call_count = completion.tool_calls.len(),
            "subagent_output_truncated"
        );
        fork_child_push(
            &params.shared.session.db,
            params.fork_run_id,
            child_messages,
            assistant_from_completion(completion),
        )?;
        params.shared.sub_agent_dec();
        return Ok(Some(build_partial_report(
            &params.agent_type.to_string(),
            &task_preview_120(params.task),
            OVERFLOW_KIND_OUTPUT_TRUNCATED,
        )));
    }
    if let Some(u) = &completion.usage {
        apply_session_usage(params.shared, u, params.llm_snap, params.event_tx, false);
    }
    if let Some(tx) = params.event_tx {
        try_send_fork_overlay_event(
            &params.shared.fork_stream_subs,
            tx,
            Event::AssistantSegmentComplete {
                segment_index: params.inner_turn,
                fork_run_id: Some(params.fork_run_id.to_string()),
            },
        );
    }
    Ok(None)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::engine::session_llm::{build_chat_client, read_session_llm};
    use crate::subagent::params::SubagentLlmFetchParams;
    use crate::test_env::StripDeepseekApiKey;
    use crate::EngineConfig;
    use novel_config::{save_agent_api_config, AgentApiConfig};
    use tempfile::TempDir;
    use wiremock::matchers::{method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    #[test]
    fn subagent_offline_fetch_uses_offline_complete() {
        let msgs = vec![LlmChatMessage {
            role: "user".into(),
            content: "audit".into(),
            tool_call_id: None,
            tool_calls: None,
            reasoning_content: None,
        }];
        let fetch = subagent_offline_fetch(&msgs);
        match fetch {
            SubagentLlmFetch::Completion {
                completion,
                fork_dispatch,
            } => {
                assert!(fork_dispatch.is_none());
                assert!(completion.content.is_some());
            }
            _ => panic!("expected offline completion"),
        }
    }

    #[tokio::test(flavor = "current_thread")]
    async fn fetch_subagent_offline_when_llm_none() {
        let tmp = TempDir::new().unwrap();
        std::fs::create_dir_all(tmp.path().join("skills")).unwrap();
        let engine = crate::AgentEngine::new(EngineConfig {
            project_root: tmp.path().to_path_buf(),
            settings_path: tmp.path().join("settings.json"),
            db_path: tmp.path().join("state.db"),
            skills_dir: tmp.path().join("skills"),
            global_config_path: tmp.path().join(".novel-agent/api_config.json"),
        })
        .unwrap();
        let shared = engine.shared.clone();
        let snap = read_session_llm(&shared);
        let mut llm = None;
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
        let llm_msgs = crate::message::to_llm_messages(&child_messages);
        let params = SubagentLlmFetchParams {
            shared: &shared,
            agent_type: crate::AgentType::KnowledgeAuditor,
            task: "audit",
            fork_run_id: &fork_run_id,
            llm_msgs: &llm_msgs,
            active_schemas: &[],
            llm_snap: &snap,
            event_tx: None,
            message_snapshot: &[],
            inner_turn: 1,
        };
        let fetch = fetch_subagent_llm_completion(&mut llm, &mut child_messages, &params)
            .await
            .unwrap();
        assert!(matches!(
            fetch,
            SubagentLlmFetch::Completion {
                fork_dispatch: None,
                ..
            }
        ));
    }

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
        let llm_msgs = crate::message::to_llm_messages(&child_messages);
        let params = SubagentLlmFetchParams {
            shared: &shared,
            agent_type: crate::AgentType::KnowledgeAuditor,
            task: "audit",
            fork_run_id: &fork_run_id,
            llm_msgs: &llm_msgs,
            active_schemas: &[],
            llm_snap: &snap,
            event_tx: None,
            message_snapshot: &[],
            inner_turn: 1,
        };
        let fetch = fetch_subagent_llm_completion(&mut llm, &mut child_messages, &params)
            .await
            .unwrap();
        assert!(matches!(fetch, SubagentLlmFetch::Completion { .. }));
        std::env::remove_var("DEEPSEEK_API_BASE");
    }
}
