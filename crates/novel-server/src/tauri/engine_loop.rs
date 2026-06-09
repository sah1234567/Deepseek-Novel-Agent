//! Main event loop: dispatches engine commands (run, resume, approve, deny, answer, set_mode,
//! enable_unattended, exit) to `novel_core::AgentEngine` and emits UI events via Tauri channels.

use crate::AppConfig;
use novel_config::ensure_work_under_works;
use novel_core::{
    AbortController, AgentEngine, AgentError, EngineStatus, Event, ForkStreamSubscriptions,
    TerminalReason,
};
use novel_state::SessionTodo;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use tokio::sync::{mpsc, oneshot, RwLock};

fn sync_turn_in_progress_flag(flag: &AtomicBool, engine: &AgentEngine) {
    flag.store(engine.is_turn_in_progress(), Ordering::Release);
}

#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct WorkSummary {
    pub name: String,
    pub path: String,
    pub initialized: bool,
}

#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AppStatus {
    pub session_id: String,
    pub permission_mode: String,
    pub hook_running: bool,
    pub pending_user_question: bool,
    pub turn_in_progress: bool,
    pub turn_number: u32,
    pub project_initialized: bool,
    pub has_interruptible_tool_in_progress: bool,
    pub todos: Vec<SessionTodo>,
    pub session_cache_hit: i64,
    pub session_cache_miss: i64,
    pub session_completion: i64,
    pub context_tokens: i64,
    pub active_work_name: String,
}

pub enum EngineCommand {
    SendMessage {
        content: String,
        model: Option<String>,
        event_tx: Option<mpsc::UnboundedSender<Event>>,
        reply: oneshot::Sender<Result<TerminalReason, String>>,
    },
    ApproveTool {
        tool_call_id: String,
        event_tx: Option<mpsc::UnboundedSender<Event>>,
        reply: oneshot::Sender<Result<(), String>>,
    },
    DenyTool {
        tool_call_id: String,
        reason: Option<String>,
        event_tx: Option<mpsc::UnboundedSender<Event>>,
        reply: oneshot::Sender<Result<(), String>>,
    },
    AnswerQuestion {
        tool_call_id: String,
        answers: serde_json::Value,
        event_tx: Option<mpsc::UnboundedSender<Event>>,
        reply: oneshot::Sender<Result<TerminalReason, String>>,
    },
    GetStatus {
        reply: oneshot::Sender<Result<AppStatus, String>>,
    },
    SetPermissionMode {
        mode: String,
        reply: oneshot::Sender<Result<(), String>>,
    },
    ResumeSession {
        session_id: String,
        reply: oneshot::Sender<Result<String, String>>,
    },
    CreateSession {
        reply: oneshot::Sender<Result<String, String>>,
    },
    SwitchProjectAndCreateSession {
        project_root: std::path::PathBuf,
        reply: oneshot::Sender<Result<String, String>>,
    },
}

pub type EngineCommandTx = mpsc::UnboundedSender<EngineCommand>;

fn build_app_status(engine: &AgentEngine, active_work_name: &str) -> AppStatus {
    let EngineStatus {
        session_id,
        permission_mode,
        hook_running,
        pending_user_question,
        turn_in_progress,
        turn_number,
        project_initialized,
        has_interruptible_tool_in_progress,
    } = engine.status_snapshot();
    let todos = engine.list_session_todos();
    let (hit, miss, comp, ctx) = engine.session_token_summary();
    AppStatus {
        session_id,
        permission_mode,
        hook_running,
        pending_user_question,
        turn_in_progress,
        turn_number,
        project_initialized,
        has_interruptible_tool_in_progress,
        todos,
        session_cache_hit: hit,
        session_cache_miss: miss,
        session_completion: comp,
        context_tokens: ctx,
        active_work_name: active_work_name.to_string(),
    }
}

pub fn spawn_engine_loop(
    engine: AgentEngine,
    mut cmd_rx: mpsc::UnboundedReceiver<EngineCommand>,
    config: Arc<RwLock<AppConfig>>,
    abort_controller: Arc<AbortController>,
    turn_in_progress: Arc<AtomicBool>,
    fork_stream_subs: ForkStreamSubscriptions,
) {
    tauri::async_runtime::spawn(async move {
        let mut engine = engine;
        while let Some(cmd) = cmd_rx.recv().await {
            match cmd {
                EngineCommand::SendMessage {
                    content,
                    model,
                    event_tx,
                    reply,
                } => {
                    tracing::debug!(content_len = content.len(), "engine_command SendMessage");
                    turn_in_progress.store(true, Ordering::Release);
                    engine.clear_interrupt();
                    let result = engine
                        .handle_message_with_events(&content, model.as_deref(), event_tx.as_ref())
                        .await
                        .map_err(|e: AgentError| {
                            tracing::warn!(error = %e, "engine_command SendMessage failed");
                            e.to_string()
                        });
                    engine.clear_interrupt();
                    sync_turn_in_progress_flag(&turn_in_progress, &engine);
                    let _ = reply.send(result);
                }
                EngineCommand::ApproveTool {
                    tool_call_id,
                    event_tx,
                    reply,
                } => {
                    tracing::debug!(%tool_call_id, "engine_command ApproveTool");
                    turn_in_progress.store(true, Ordering::Release);
                    let result = engine
                        .approve_tool(&tool_call_id, event_tx.as_ref())
                        .await
                        .map_err(|e: AgentError| {
                            tracing::warn!(error = %e, %tool_call_id, "engine_command ApproveTool failed");
                            e.to_string()
                        });
                    sync_turn_in_progress_flag(&turn_in_progress, &engine);
                    let _ = reply.send(result);
                }
                EngineCommand::DenyTool {
                    tool_call_id,
                    reason,
                    event_tx,
                    reply,
                } => {
                    tracing::debug!(%tool_call_id, "engine_command DenyTool");
                    turn_in_progress.store(true, Ordering::Release);
                    let result = engine
                        .deny_tool(&tool_call_id, reason, event_tx.as_ref())
                        .await
                        .map_err(|e: AgentError| {
                            tracing::warn!(error = %e, %tool_call_id, "engine_command DenyTool failed");
                            e.to_string()
                        });
                    sync_turn_in_progress_flag(&turn_in_progress, &engine);
                    let _ = reply.send(result);
                }
                EngineCommand::AnswerQuestion {
                    tool_call_id,
                    answers,
                    event_tx,
                    reply,
                } => {
                    tracing::debug!(%tool_call_id, "engine_command AnswerQuestion");
                    turn_in_progress.store(true, Ordering::Release);
                    let result = engine
                        .answer_question(&tool_call_id, answers, event_tx.as_ref())
                        .await
                        .map_err(|e: AgentError| {
                            tracing::warn!(error = %e, %tool_call_id, "engine_command AnswerQuestion failed");
                            e.to_string()
                        });
                    sync_turn_in_progress_flag(&turn_in_progress, &engine);
                    let _ = reply.send(result);
                }
                EngineCommand::GetStatus { reply } => {
                    tracing::trace!("engine_command GetStatus");
                    let work_name = config.read().await.active_work_name();
                    let status = build_app_status(&engine, &work_name);
                    let _ = reply.send(Ok(status));
                }
                EngineCommand::SetPermissionMode { mode, reply } => {
                    tracing::debug!(%mode, "engine_command SetPermissionMode");
                    let result =
                        super::session_api::parse_permission_mode(&mode).and_then(|parsed| {
                            engine
                                .apply_permission_mode_change(parsed)
                                .map_err(|e| e.to_string())
                        });
                    let _ = reply.send(result);
                }
                EngineCommand::ResumeSession { session_id, reply } => {
                    tracing::debug!(%session_id, "engine_command ResumeSession");
                    abort_controller.clear();
                    let ecfg = config.read().await.engine_config();
                    let result = match AgentEngine::resume_with_abort(
                        ecfg,
                        &session_id,
                        Arc::clone(&abort_controller),
                        Arc::clone(&fork_stream_subs),
                    ) {
                        Ok(mut e) => {
                            e.attach_fork_stream_subs(Arc::clone(&fork_stream_subs));
                            let sid = e.session_id().to_string();
                            engine = e;
                            sync_turn_in_progress_flag(&turn_in_progress, &engine);
                            Ok(sid)
                        }
                        Err(e) => {
                            tracing::warn!(error = %e, %session_id, "engine_command ResumeSession failed");
                            Err(e.to_string())
                        }
                    };
                    let _ = reply.send(result);
                }
                EngineCommand::CreateSession { reply } => {
                    tracing::debug!("engine_command CreateSession");
                    abort_controller.clear();
                    let ecfg = config.read().await.engine_config();
                    let result = match AgentEngine::new_with_abort(
                        ecfg,
                        Arc::clone(&abort_controller),
                        Arc::clone(&fork_stream_subs),
                    ) {
                        Ok(mut e) => {
                            e.attach_fork_stream_subs(Arc::clone(&fork_stream_subs));
                            let sid = e.session_id().to_string();
                            engine = e;
                            sync_turn_in_progress_flag(&turn_in_progress, &engine);
                            Ok(sid)
                        }
                        Err(e) => {
                            tracing::warn!(error = %e, "engine_command CreateSession failed");
                            Err(e.to_string())
                        }
                    };
                    let _ = reply.send(result);
                }
                EngineCommand::SwitchProjectAndCreateSession {
                    project_root,
                    reply,
                } => {
                    let project_root_display = project_root.display().to_string();
                    tracing::debug!(
                        project_root = %project_root_display,
                        "engine_command SwitchProjectAndCreateSession"
                    );
                    let ecfg = {
                        let mut cfg = config.write().await;
                        if let Err(e) = ensure_work_under_works(&cfg.works_dir, &project_root) {
                            let _ = reply.send(Err(e.to_string()));
                            continue;
                        }
                        cfg.set_active_project(project_root);
                        cfg.engine_config()
                    };
                    if let Some(parent) = ecfg.db_path.parent() {
                        let _ = std::fs::create_dir_all(parent);
                    }
                    let result = match AgentEngine::new_with_abort(
                        ecfg,
                        Arc::clone(&abort_controller),
                        Arc::clone(&fork_stream_subs),
                    ) {
                        Ok(mut e) => {
                            e.attach_fork_stream_subs(Arc::clone(&fork_stream_subs));
                            let sid = e.session_id().to_string();
                            engine = e;
                            sync_turn_in_progress_flag(&turn_in_progress, &engine);
                            Ok(sid)
                        }
                        Err(e) => {
                            tracing::warn!(
                                error = %e,
                                project_root = %project_root_display,
                                "engine_command SwitchProjectAndCreateSession failed"
                            );
                            Err(e.to_string())
                        }
                    };
                    let _ = reply.send(result);
                }
            }
        }
        tracing::error!("engine_loop_exited: command channel closed");
    });
}
