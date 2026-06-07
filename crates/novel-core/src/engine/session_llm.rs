//! Session-level LLM configuration snapshot (main Agent last API model + thinking).
//!
//! Updated at turn start (StatusBar model override), after each main LLM response, and when
//! (re)building `AgentEngine.llm`, so subagent drain reads the same model/thinking as the main session.
//!
//! API key resolution: `novel_config::resolve_agent_api_key` (env `DEEPSEEK_API_KEY` >
//! `{agent_root}/.novel-agent/api_config.json`), then [`ChatClient::from_api_key_or_env`].

use crate::{EngineShared, Event};
use novel_deepseek::{ChatClient, TokenUsage};
use novel_logging::LogEvent;
use std::path::Path;
use std::sync::{Arc, RwLock};
use tokio::sync::mpsc;

#[derive(Debug, Clone)]
pub struct SessionLlmSnapshot {
    pub model: String,
    pub thinking_enabled: bool,
}

impl SessionLlmSnapshot {
    pub fn from_settings(settings: &novel_config::ProjectSettings) -> Self {
        Self {
            model: settings.model.model.clone(),
            thinking_enabled: settings.model.thinking_enabled,
        }
    }
}

pub type SessionLlm = Arc<RwLock<SessionLlmSnapshot>>;

pub fn new_session_llm(settings: &novel_config::ProjectSettings) -> SessionLlm {
    Arc::new(RwLock::new(SessionLlmSnapshot::from_settings(settings)))
}

/// Persist per-session model/thinking snapshot (used by `build_chat_client` and subagent drain).
pub fn write_session_llm(shared: &EngineShared, snap: SessionLlmSnapshot) {
    if let Ok(mut guard) = shared.session_llm.write() {
        *guard = snap;
    }
}

pub fn read_session_llm(shared: &EngineShared) -> SessionLlmSnapshot {
    shared
        .session_llm
        .read()
        .map(|g| g.clone())
        .unwrap_or_else(|_| SessionLlmSnapshot::from_settings(&shared.settings))
}

/// Resolve agent API credentials then construct a `ChatClient` for the given snapshot.
pub fn build_chat_client(
    snap: &SessionLlmSnapshot,
    global_config_path: &Path,
) -> Option<ChatClient> {
    let api_key = novel_config::resolve_agent_api_key(global_config_path);
    let api_base = novel_config::resolve_agent_api_base(global_config_path);
    ChatClient::from_api_key_or_env(
        api_key.as_deref(),
        &api_base,
        &snap.model,
        snap.thinking_enabled,
    )
}

/// Single path for session token DB + audit + `SessionTokensUpdated` after LLM usage.
/// Pass `update_context_snapshot: false` for fork subagent billing so StatusBar keeps the parent snapshot.
pub fn apply_session_usage(
    shared: &EngineShared,
    usage: &TokenUsage,
    snap: &SessionLlmSnapshot,
    event_tx: Option<&mpsc::UnboundedSender<Event>>,
    update_context_snapshot: bool,
) {
    if let Err(e) = shared.session.db.accumulate_session_tokens(
        &shared.session.id,
        usage.cache_hit_tokens,
        usage.cache_miss_tokens,
        usage.completion_tokens,
        &snap.model,
        update_context_snapshot,
    ) {
        tracing::warn!(
            session_id = %shared.session.id,
            error = %e,
            "accumulate_session_tokens failed; skipping SessionTokensUpdated emit"
        );
        return;
    }
    shared.audit_log(&LogEvent::TokenAudit {
        session_id: shared.session.id.clone(),
        cache_hit_tokens: usage.cache_hit_tokens,
        cache_miss_tokens: usage.cache_miss_tokens,
        completion_tokens: usage.completion_tokens,
    });
    emit_session_tokens_updated(shared, event_tx);
}

fn emit_session_tokens_updated(
    shared: &EngineShared,
    event_tx: Option<&mpsc::UnboundedSender<Event>>,
) {
    let Some(tx) = event_tx else {
        return;
    };
    let Ok(Some(s)) = shared.session.db.get_session(&shared.session.id) else {
        return;
    };
    let _ = tx.send(Event::SessionTokensUpdated {
        cache_hit_tokens: s.cache_hit_tokens,
        cache_miss_tokens: s.cache_miss_tokens,
        completion_tokens: s.completion_tokens,
        context_tokens: s.context_tokens,
    });
}
