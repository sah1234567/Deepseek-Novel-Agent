use crate::AppConfig;
use novel_core::{
    run_subagent_async, AgentEngine, AgentError, AgentType, EngineStatus, Event,
    TerminalReason,
};
use novel_config::ensure_work_under_works;
use novel_state::SessionTodo;
use novel_tools::PermissionMode;
use std::sync::Arc;
use tokio::sync::{mpsc, oneshot, RwLock};

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
    pub turn_number: u32,
    pub project_initialized: bool,
    pub has_interruptible_tool_in_progress: bool,
    pub todos: Vec<SessionTodo>,
    pub session_cache_hit: i64,
    pub session_cache_miss: i64,
    pub session_completion: i64,
    pub session_total_tokens: i64,
    pub project_root: String,
    pub active_work_name: String,
}

pub enum EngineCommand {
    SendMessage {
        content: String,
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
    ForkSubAgent {
        agent_type: AgentType,
        task: String,
        event_tx: Option<mpsc::UnboundedSender<Event>>,
        reply: oneshot::Sender<Result<(String, String), String>>,
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
        turn_number,
        project_initialized,
        has_interruptible_tool_in_progress,
    } = engine.status_snapshot();
    let todos = engine
        .shared.session
        .db
        .list_session_todos(&session_id)
        .unwrap_or_default();
    let (hit, miss, comp) = engine.session_token_summary();
    let total = hit + miss + comp;
    AppStatus {
        session_id,
        permission_mode,
        hook_running,
        pending_user_question,
        turn_number,
        project_initialized,
        has_interruptible_tool_in_progress,
        todos,
        session_cache_hit: hit,
        session_cache_miss: miss,
        session_completion: comp,
        session_total_tokens: total,
        project_root: engine.shared.session.project_root.display().to_string(),
        active_work_name: active_work_name.to_string(),
    }
}

fn parse_permission_mode(mode: &str) -> Result<PermissionMode, String> {
    match mode {
        "normal" => Ok(PermissionMode::Normal),
        "plan" => Ok(PermissionMode::Plan),
        "auto" => Ok(PermissionMode::Auto),
        "unattended" => Ok(PermissionMode::Unattended),
        other => Err(format!("invalid permission mode: {other}")),
    }
}

pub fn spawn_engine_loop(
    engine: AgentEngine,
    mut cmd_rx: mpsc::UnboundedReceiver<EngineCommand>,
    config: Arc<RwLock<AppConfig>>,
) {
    tauri::async_runtime::spawn(async move {
        let mut engine = engine;
        while let Some(cmd) = cmd_rx.recv().await {
            match cmd {
                EngineCommand::SendMessage {
                    content,
                    event_tx,
                    reply,
                } => {
                    tracing::debug!(content_len = content.len(), "engine_command SendMessage");
                    engine.clear_interrupt();
                    let result = engine
                        .handle_message_with_events(&content, event_tx.as_ref())
                        .await
                        .map_err(|e: AgentError| {
                            tracing::warn!(error = %e, "engine_command SendMessage failed");
                            e.to_string()
                        });
                    engine.clear_interrupt();
                    let _ = reply.send(result);
                }
                EngineCommand::ApproveTool {
                    tool_call_id,
                    event_tx,
                    reply,
                } => {
                    tracing::debug!(%tool_call_id, "engine_command ApproveTool");
                    let result = engine
                        .approve_tool(&tool_call_id, event_tx.as_ref())
                        .await
                        .map_err(|e: AgentError| {
                            tracing::warn!(error = %e, %tool_call_id, "engine_command ApproveTool failed");
                            e.to_string()
                        });
                    let _ = reply.send(result);
                }
                EngineCommand::DenyTool {
                    tool_call_id,
                    reason,
                    event_tx,
                    reply,
                } => {
                    tracing::debug!(%tool_call_id, "engine_command DenyTool");
                    let result = engine
                        .deny_tool(&tool_call_id, reason, event_tx.as_ref())
                        .await
                        .map_err(|e: AgentError| {
                            tracing::warn!(error = %e, %tool_call_id, "engine_command DenyTool failed");
                            e.to_string()
                        });
                    let _ = reply.send(result);
                }
                EngineCommand::ForkSubAgent {
                    agent_type,
                    task,
                    event_tx,
                    reply,
                } => {
                    tracing::debug!(?agent_type, task_len = task.len(), "engine_command ForkSubAgent");
                    let agent_id = uuid::Uuid::new_v4().to_string();
                    // Debug-only IPC: fire-and-forget subagent; does not inject into parent session.
                    let fork_run_id = match novel_core::fork_transcript::create_fork_run(
                        &engine.shared.session.db,
                        &engine.shared.session.id,
                        engine.turn_number as i32,
                        &agent_type.to_string(),
                        &task,
                        "tool",
                    ) {
                        Ok(id) => id,
                        Err(e) => {
                            let _ = reply.send(Err(e.to_string()));
                            continue;
                        }
                    };
                    let shared = engine.shared.clone();
                    let tx = engine.subagent_result_tx.clone();
                    engine.sub_agent_inc();
                    tokio::spawn(async move {
                        let output = run_subagent_async(
                            shared,
                            agent_type,
                            task,
                            fork_run_id,
                            event_tx,
                        )
                        .await
                        .unwrap_or_else(|e| format!("子 Agent 错误: {e}"));
                        let _ = tx.send((agent_type, output));
                    });
                    let _ = reply.send(Ok((
                        agent_id,
                        "子 Agent 已在后台启动（debug IPC；主会话不自动 inject）".into(),
                    )));
                }
                EngineCommand::AnswerQuestion {
                    tool_call_id,
                    answers,
                    event_tx,
                    reply,
                } => {
                    tracing::debug!(%tool_call_id, "engine_command AnswerQuestion");
                    let result = engine
                        .answer_question(&tool_call_id, answers, event_tx.as_ref())
                        .await
                        .map_err(|e: AgentError| {
                            tracing::warn!(error = %e, %tool_call_id, "engine_command AnswerQuestion failed");
                            e.to_string()
                        });
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
                    let result = parse_permission_mode(&mode).map(|parsed| {
                        engine.set_permission_mode_override(parsed);
                    });
                    let _ = reply.send(result);
                }
                EngineCommand::ResumeSession {
                    session_id,
                    reply,
                } => {
                    tracing::debug!(%session_id, "engine_command ResumeSession");
                    let ecfg = config.read().await.engine_config();
                    let abort = Arc::clone(&engine.shared.abort_controller);
                    let result = match AgentEngine::resume_with_abort(
                        ecfg,
                        &session_id,
                        abort,
                    ) {
                        Ok(e) => {
                            let sid = e.shared.session.id.clone();
                            engine = e;
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
                    let ecfg = config.read().await.engine_config();
                    let result = match AgentEngine::new(ecfg) {
                        Ok(e) => {
                            let sid = e.shared.session.id.clone();
                            engine = e;
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
                    let result = match AgentEngine::new(ecfg) {
                        Ok(e) => {
                            let sid = e.shared.session.id.clone();
                            engine = e;
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
    });
}
