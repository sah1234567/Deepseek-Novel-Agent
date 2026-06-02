//! Shared stream-cancel finalization for main session and fork subagents.
//! Token usage after cancel uses `session_llm::apply_session_usage`; prompt estimate uses `build_chat_client`.

use crate::message_bridge::assistant_from_completion;
use crate::messages::yield_missing_tool_result_blocks;
use crate::session_llm::{apply_session_usage, build_chat_client, SessionLlmSnapshot};
use crate::{AgentError, ChatMessage, EngineShared, Event};
use novel_deepseek::{LlmChatMessage, LlmCompletion, TokenUsage};
use novel_tools::format_interrupted_tool_result;
use std::time::Duration;
use tokio::sync::mpsc;

pub trait TurnFinalizeSink: Send {
    fn persist_partial_assistant(&mut self, completion: &LlmCompletion) -> Result<(), AgentError>;
    fn push_tool_stub(&mut self, stub: ChatMessage) -> Result<(), AgentError>;
}

pub struct MainSessionSink<'a> {
    pub engine: &'a mut crate::AgentEngine,
}

impl TurnFinalizeSink for MainSessionSink<'_> {
    fn persist_partial_assistant(&mut self, completion: &LlmCompletion) -> Result<(), AgentError> {
        let assistant = assistant_from_completion(completion);
        if assistant.content.is_empty() && assistant.tool_calls.is_none() {
            return Ok(());
        }
        self.engine.persist_message_alloc(&assistant)?;
        self.engine.messages.push(assistant);
        Ok(())
    }

    fn push_tool_stub(&mut self, stub: ChatMessage) -> Result<(), AgentError> {
        self.engine.persist_message_alloc(&stub)?;
        self.engine.messages.push(stub);
        Ok(())
    }
}

pub struct ForkTranscriptSink<'a> {
    pub db: &'a novel_state::Database,
    pub fork_run_id: &'a str,
    pub child: &'a mut Vec<ChatMessage>,
}

impl TurnFinalizeSink for ForkTranscriptSink<'_> {
    fn persist_partial_assistant(&mut self, completion: &LlmCompletion) -> Result<(), AgentError> {
        let assistant = assistant_from_completion(completion);
        if assistant.content.is_empty() && assistant.tool_calls.is_none() {
            return Ok(());
        }
        crate::fork_transcript::persist_fork_message(self.db, self.fork_run_id, &assistant)?;
        self.child.push(assistant);
        Ok(())
    }

    fn push_tool_stub(&mut self, stub: ChatMessage) -> Result<(), AgentError> {
        crate::fork_transcript::persist_fork_message(self.db, self.fork_run_id, &stub)?;
        self.child.push(stub);
        Ok(())
    }
}

pub struct FinalizeStreamCancelParams<'a> {
    pub sink: &'a mut dyn TurnFinalizeSink,
    pub partial: LlmCompletion,
    pub llm_messages: Vec<LlmChatMessage>,
    pub tool_schemas: Vec<(String, String, serde_json::Value)>,
    pub background_usage: Option<tokio::sync::oneshot::Receiver<Option<TokenUsage>>>,
    pub llm_snap: SessionLlmSnapshot,
    pub shared: EngineShared,
    pub event_tx: Option<&'a mpsc::UnboundedSender<Event>>,
}

pub async fn measure_context_usage(
    snap: &SessionLlmSnapshot,
    global_config_path: &std::path::Path,
    messages: &[LlmChatMessage],
    tools: &[(String, String, serde_json::Value)],
    initial: Option<TokenUsage>,
) -> Option<TokenUsage> {
    let client = build_chat_client(snap, global_config_path)?;
    client.measure_prompt_usage(messages, tools, initial).await
}

pub async fn finalize_stream_cancel(
    params: FinalizeStreamCancelParams<'_>,
) -> Result<Option<TokenUsage>, AgentError> {
    let usage = if let Some(rx) = params.background_usage {
        match tokio::time::timeout(Duration::from_secs(1), rx).await {
            Ok(Ok(Some(u))) => Some(u),
            _ => measure_context_usage(
                &params.llm_snap,
                &params.shared.global_config_path,
                &params.llm_messages,
                &params.tool_schemas,
                params.partial.usage.clone(),
            )
            .await
            .or_else(|| params.partial.usage.clone()),
        }
    } else {
        measure_context_usage(
            &params.llm_snap,
            &params.shared.global_config_path,
            &params.llm_messages,
            &params.tool_schemas,
            params.partial.usage.clone(),
        )
        .await
        .or_else(|| params.partial.usage.clone())
    };

    if let Some(ref u) = usage {
        apply_session_usage(&params.shared, u, &params.llm_snap, params.event_tx);
    } else {
        let _ = params
            .shared
            .session
            .db
            .touch_last_active_at(&params.shared.session.id);
    }

    params.sink.persist_partial_assistant(&params.partial)?;

    let assistant = assistant_from_completion(&params.partial);
    for stub in append_interrupted_tool_stubs(&assistant) {
        params.sink.push_tool_stub(stub)?;
    }

    Ok(usage)
}

pub fn append_interrupted_tool_stubs(assistant: &ChatMessage) -> Vec<ChatMessage> {
    let mut blocks = yield_missing_tool_result_blocks(assistant, "");
    for block in &mut blocks {
        if let Some(id) = &block.tool_call_id {
            let name = assistant
                .tool_calls
                .as_ref()
                .and_then(|tcs| tcs.iter().find(|tc| &tc.id == id))
                .map(|tc| tc.name.as_str())
                .unwrap_or("tool");
            block.content = format_interrupted_tool_result(name, id);
        }
    }
    blocks
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::ToolCallRecord;

    #[tokio::test]
    async fn finalize_stream_cancel_persists_partial_and_stubs() {
        use crate::interrupt_finalize::{FinalizeStreamCancelParams, MainSessionSink};
        use crate::test_env::StripDeepseekApiKey;
        use novel_deepseek::{LlmChatMessage, LlmCompletion};
        use tempfile::TempDir;

        let _offline = StripDeepseekApiKey::new();
        let tmp = TempDir::new().unwrap();
        std::fs::create_dir_all(tmp.path().join("skills")).unwrap();
        let cfg = crate::EngineConfig {
            project_root: tmp.path().to_path_buf(),
            settings_path: tmp.path().join("settings.json"),
            db_path: tmp.path().join("state.db"),
            skills_dir: tmp.path().join("skills"),
            global_config_path: tmp.path().join(".novel-agent/api_config.json"),
        };
        let mut engine = crate::AgentEngine::new(cfg).unwrap();
        let partial = LlmCompletion {
            content: Some("partial".into()),
            tool_calls: vec![novel_deepseek::LlmToolCall {
                id: "tc1".into(),
                name: "Read".into(),
                arguments: "{}".into(),
            }],
            usage: None,
            reasoning_content: None,
            stop_reason: None,
        };
        let shared = engine.shared.clone();
        let llm_snap = crate::session_llm::read_session_llm(&shared);
        let mut sink = MainSessionSink {
            engine: &mut engine,
        };
        let params = FinalizeStreamCancelParams {
            sink: &mut sink,
            partial,
            llm_messages: vec![LlmChatMessage {
                role: "user".into(),
                content: "hi".into(),
                tool_call_id: None,
                tool_calls: None,
                reasoning_content: None,
            }],
            tool_schemas: vec![],
            background_usage: None,
            llm_snap,
            shared,
            event_tx: None,
        };
        finalize_stream_cancel(params).await.unwrap();
        assert!(engine
            .messages
            .iter()
            .any(|m| m.role == "assistant" && m.content == "partial"));
        assert!(engine.messages.iter().any(|m| m.role == "tool"));
    }

    #[test]
    fn append_interrupted_tool_stubs_fills_missing_results() {
        let assistant = ChatMessage {
            role: "assistant".into(),
            content: String::new(),
            tool_calls: Some(vec![ToolCallRecord {
                id: "tc1".into(),
                name: "Read".into(),
                arguments: serde_json::json!({}),
            }]),
            ..Default::default()
        };
        let stubs = append_interrupted_tool_stubs(&assistant);
        assert_eq!(stubs.len(), 1);
        assert_eq!(stubs[0].role, "tool");
        assert_eq!(stubs[0].tool_call_id.as_deref(), Some("tc1"));
        assert!(stubs[0].content.contains("Read"));
    }
}
