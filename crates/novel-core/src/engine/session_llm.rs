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
///
/// Always emits `SessionTokensUpdated` on successful accumulate (main and subagent).
/// Pass `update_context_snapshot: false` for fork subagent billing: billing counters still
/// accumulate and the event is sent, but DB `context_tokens` keeps the parent snapshot.
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
    emit_session_tokens_updated(shared, event_tx, update_context_snapshot);
}

fn emit_session_tokens_updated(
    shared: &EngineShared,
    event_tx: Option<&mpsc::UnboundedSender<Event>>,
    context_snapshot_updated: bool,
) {
    let Some(tx) = event_tx else {
        return;
    };
    let Ok(Some(s)) = shared.session.db.get_session(&shared.session.id) else {
        return;
    };
    tracing::debug!(
        session_id = %shared.session.id,
        cache_hit = s.cache_hit_tokens,
        cache_miss = s.cache_miss_tokens,
        completion = s.completion_tokens,
        context = s.context_tokens,
        context_snapshot_updated,
        "session_tokens_updated_emit"
    );
    let _ = tx.send(Event::SessionTokensUpdated {
        cache_hit_tokens: s.cache_hit_tokens,
        cache_miss_tokens: s.cache_miss_tokens,
        completion_tokens: s.completion_tokens,
        context_tokens: s.context_tokens,
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::engine::types::{AgentEngine, EngineConfig};
    use tempfile::TempDir;

    fn test_config(tmp: &TempDir) -> EngineConfig {
        std::fs::create_dir_all(tmp.path().join("skills")).unwrap();
        EngineConfig {
            project_root: tmp.path().to_path_buf(),
            settings_path: tmp.path().join("settings.json"),
            db_path: tmp.path().join("state.db"),
            skills_dir: tmp.path().join("skills"),
            global_config_path: tmp.path().join(".novel-agent/api_config.json"),
        }
    }

    #[test]
    fn fork_usage_emits_billing_without_context_snapshot_change() {
        let tmp = TempDir::new().unwrap();
        let engine = AgentEngine::new(test_config(&tmp)).unwrap();
        let (tx, mut rx) = mpsc::unbounded_channel();
        let usage = TokenUsage {
            cache_hit_tokens: 1,
            cache_miss_tokens: 2,
            completion_tokens: 3,
            reasoning_tokens: 0,
        };
        let snap = read_session_llm(&engine.shared);
        let session_id = engine.shared.session.id.clone();

        apply_session_usage(&engine.shared, &usage, &snap, Some(&tx), true);
        let main_evt = rx.try_recv().expect("main agent emit");
        assert!(matches!(
            main_evt,
            Event::SessionTokensUpdated {
                cache_hit_tokens: 1,
                cache_miss_tokens: 2,
                completion_tokens: 3,
                context_tokens: 6,
            }
        ));
        let after_main = engine
            .shared
            .session
            .db
            .get_session(&session_id)
            .unwrap()
            .unwrap();
        assert_eq!(after_main.context_tokens, 6);

        apply_session_usage(&engine.shared, &usage, &snap, Some(&tx), false);
        let fork_evt = rx.try_recv().expect("subagent emit");
        assert!(matches!(
            fork_evt,
            Event::SessionTokensUpdated {
                cache_hit_tokens: 2,
                cache_miss_tokens: 4,
                completion_tokens: 6,
                context_tokens: 6,
            }
        ));
        let after_fork = engine
            .shared
            .session
            .db
            .get_session(&session_id)
            .unwrap()
            .unwrap();
        assert_eq!(after_fork.cache_hit_tokens, 2);
        assert_eq!(after_fork.context_tokens, 6);
    }
}
