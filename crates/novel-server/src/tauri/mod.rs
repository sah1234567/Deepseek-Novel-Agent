//! Tauri IPC bridge (optional `tauri` feature — compiles without npm/frontend).
//!
//! Engine access goes through a single-task command loop (`engine_loop`).
//! Do not await user approval while holding any engine lock — approval uses `ApproveTool` commands.

use tauri::Emitter;

mod dto;
mod engine_loop;
mod event_payload;
mod events;
mod session_api;
mod state;

pub use dto::{
    build_session_transcript, fork_messages_to_ui, stored_messages_to_ui, SessionTranscript,
    SessionTranscriptArchive, UiContentBlock, UiMessage,
};
pub use engine_loop::{AppStatus, WorkSummary};
pub use events::{
    emit_core_event, StreamChunkPayload, SubAgentCompletePayload, ToolCallRequestPayload,
    TurnCompletePayload,
};
pub use state::{AppState, CommandContext};

use engine_loop::EngineCommand;
use tokio::sync::{mpsc, oneshot};

/// Spawn a background task forwarding core events to the Tauri frontend.
pub fn spawn_event_forwarder(
    ctx: &CommandContext,
    message_id: Option<String>,
) -> mpsc::UnboundedSender<novel_core::Event> {
    let (event_tx, mut event_rx) = mpsc::unbounded_channel();
    let app_handle = ctx.app_handle.clone();
    let current_message_id = std::sync::Arc::clone(&ctx.current_message_id);
    tauri::async_runtime::spawn(async move {
        let mid = if let Some(id) = message_id {
            id
        } else {
            current_message_id.read().await.clone()
        };
        while let Some(event) = event_rx.recv().await {
            emit_core_event(&app_handle, event, &mid);
        }
    });
    event_tx
}

pub async fn send_message(
    ctx: &CommandContext,
    content: String,
    model: Option<String>,
) -> Result<String, String> {
    if content.trim().is_empty() {
        return Err("empty message".into());
    }
    let msg_id = uuid::Uuid::new_v4().to_string();
    *ctx.current_message_id.write().await = msg_id.clone();

    let event_tx = spawn_event_forwarder(ctx, Some(msg_id.clone()));

    let (reply_tx, reply_rx) = oneshot::channel();
    ctx.cmd_tx
        .send(EngineCommand::SendMessage {
            content,
            model,
            event_tx: Some(event_tx),
            reply: reply_tx,
        })
        .map_err(|_| "engine loop stopped".to_string())?;
    let _ = reply_rx
        .await
        .map_err(|_| "engine loop stopped".to_string())??;
    Ok(msg_id)
}

pub async fn interrupt(ctx: &CommandContext, reason: Option<String>) -> Result<(), String> {
    let r = reason
        .as_deref()
        .map(novel_core::InterruptReason::parse_reason)
        .unwrap_or(novel_core::InterruptReason::UserCancel);
    ctx.abort_controller.request(r);
    Ok(())
}

pub async fn approve_tool(ctx: &CommandContext, tool_call_id: String) -> Result<(), String> {
    let event_tx = spawn_event_forwarder(ctx, None);
    let (reply_tx, reply_rx) = oneshot::channel();
    ctx.cmd_tx
        .send(EngineCommand::ApproveTool {
            tool_call_id,
            event_tx: Some(event_tx),
            reply: reply_tx,
        })
        .map_err(|_| "engine loop stopped".to_string())?;
    reply_rx
        .await
        .map_err(|_| "engine loop stopped".to_string())?
}

pub async fn deny_tool(
    ctx: &CommandContext,
    tool_call_id: String,
    reason: Option<String>,
) -> Result<(), String> {
    let event_tx = spawn_event_forwarder(ctx, None);
    let (reply_tx, reply_rx) = oneshot::channel();
    ctx.cmd_tx
        .send(EngineCommand::DenyTool {
            tool_call_id,
            reason,
            event_tx: Some(event_tx),
            reply: reply_tx,
        })
        .map_err(|_| "engine loop stopped".to_string())?;
    reply_rx
        .await
        .map_err(|_| "engine loop stopped".to_string())?
}

pub async fn get_fork_messages(
    ctx: &CommandContext,
    run_id: String,
) -> Result<Vec<UiMessage>, String> {
    let db = {
        let cfg = ctx.config.read().await;
        novel_state::Database::open(cfg.db_path()).map_err(|e| e.to_string())?
    };
    let stored = db.get_fork_messages(&run_id).map_err(|e| e.to_string())?;
    Ok(fork_messages_to_ui(&stored))
}

pub async fn answer_question(
    ctx: &CommandContext,
    tool_call_id: String,
    answers: serde_json::Value,
) -> Result<(), String> {
    let event_tx = spawn_event_forwarder(ctx, None);

    let (reply_tx, reply_rx) = oneshot::channel();
    ctx.cmd_tx
        .send(EngineCommand::AnswerQuestion {
            tool_call_id,
            answers,
            event_tx: Some(event_tx),
            reply: reply_tx,
        })
        .map_err(|_| "engine loop stopped".to_string())?;
    let _ = reply_rx
        .await
        .map_err(|_| "engine loop stopped".to_string())??;
    Ok(())
}

pub async fn get_app_status(ctx: &CommandContext) -> Result<AppStatus, String> {
    let (reply_tx, reply_rx) = oneshot::channel();
    ctx.cmd_tx
        .send(EngineCommand::GetStatus { reply: reply_tx })
        .map_err(|_| "engine loop stopped".to_string())?;
    reply_rx
        .await
        .map_err(|_| "engine loop stopped".to_string())?
}

pub async fn set_permission_mode(ctx: &CommandContext, mode: String) -> Result<(), String> {
    let (reply_tx, reply_rx) = oneshot::channel();
    ctx.cmd_tx
        .send(EngineCommand::SetPermissionMode {
            mode: mode.clone(),
            reply: reply_tx,
        })
        .map_err(|_| "engine loop stopped".to_string())?;
    reply_rx
        .await
        .map_err(|_| "engine loop stopped".to_string())??;
    let _ = ctx.app_handle.emit(
        "permission-mode-changed",
        serde_json::json!({ "mode": mode }),
    );
    Ok(())
}

pub async fn init_novel_project(ctx: &CommandContext) -> Result<(), String> {
    let (work, templates) = {
        let cfg = ctx.config.read().await;
        (cfg.active_project.clone(), cfg.templates_dir())
    };
    novel_knowledge::init_project_scaffold(&work, templates.as_path()).map_err(|e| e.to_string())
}

pub fn list_works(works_dir: &std::path::Path) -> Result<Vec<WorkSummary>, String> {
    if !works_dir.is_dir() {
        return Ok(Vec::new());
    }
    std::fs::read_dir(works_dir).map_err(|e| e.to_string())?;
    let mut works = session_api::list_work_dirs(works_dir)
        .into_iter()
        .map(|name| {
            let path = works_dir.join(&name);
            let initialized = path.join("AGENTS.md").is_file() || path.join("knowledge").is_dir();
            WorkSummary {
                path: path.to_string_lossy().into_owned(),
                name,
                initialized,
            }
        })
        .collect::<Vec<_>>();
    works.sort_by(|a, b| a.name.cmp(&b.name));
    Ok(works)
}

pub async fn list_works_cmd(ctx: &CommandContext) -> Result<Vec<WorkSummary>, String> {
    let cfg = ctx.config.read().await;
    list_works(&cfg.works_dir)
}

pub async fn create_work(ctx: &CommandContext, name: String) -> Result<String, String> {
    use novel_config::work_path;
    let (work, templates) = {
        let cfg = ctx.config.read().await;
        (
            work_path(&cfg.agent_root, &name).map_err(|e| e.to_string())?,
            cfg.templates_dir(),
        )
    };
    if !work.exists() {
        novel_knowledge::init_project_scaffold(&work, templates.as_path())
            .map_err(|e| e.to_string())?;
    }
    switch_project_and_create_session(ctx, work).await
}

pub async fn open_work(ctx: &CommandContext, name: String) -> Result<String, String> {
    let work = {
        let cfg = ctx.config.read().await;
        novel_config::work_path(&cfg.agent_root, &name).map_err(|e| e.to_string())?
    };
    if !work.exists() {
        return Err(format!("work not found: {name}"));
    }
    switch_project_and_create_session(ctx, work).await
}

async fn switch_project_and_create_session(
    ctx: &CommandContext,
    project_root: std::path::PathBuf,
) -> Result<String, String> {
    let (reply_tx, reply_rx) = oneshot::channel();
    ctx.cmd_tx
        .send(EngineCommand::SwitchProjectAndCreateSession {
            project_root,
            reply: reply_tx,
        })
        .map_err(|_| "engine loop stopped".to_string())?;
    let sid = reply_rx
        .await
        .map_err(|_| "engine loop stopped".to_string())??;
    let _ = ctx
        .app_handle
        .emit("session-resumed", serde_json::json!({ "sessionId": sid }));
    Ok(sid)
}

pub async fn create_session(ctx: &CommandContext) -> Result<String, String> {
    let (reply_tx, reply_rx) = oneshot::channel();
    ctx.cmd_tx
        .send(EngineCommand::CreateSession { reply: reply_tx })
        .map_err(|_| "engine loop stopped".to_string())?;
    let sid = reply_rx
        .await
        .map_err(|_| "engine loop stopped".to_string())??;
    let _ = ctx
        .app_handle
        .emit("session-resumed", serde_json::json!({ "sessionId": sid }));
    Ok(sid)
}

pub async fn resume_session(ctx: &CommandContext, session_id: String) -> Result<String, String> {
    let (reply_tx, reply_rx) = oneshot::channel();
    ctx.cmd_tx
        .send(EngineCommand::ResumeSession {
            session_id: session_id.clone(),
            reply: reply_tx,
        })
        .map_err(|_| "engine loop stopped".to_string())?;
    let sid = reply_rx
        .await
        .map_err(|_| "engine loop stopped".to_string())??;
    let _ = ctx
        .app_handle
        .emit("session-resumed", serde_json::json!({ "sessionId": sid }));
    Ok(sid)
}

pub async fn get_session_transcript(
    ctx: &CommandContext,
    session_id: Option<String>,
) -> Result<SessionTranscript, String> {
    let sid = match session_id {
        Some(id) => id,
        None => get_app_status(ctx).await?.session_id,
    };
    let db = {
        let cfg = ctx.config.read().await;
        novel_state::Database::open(cfg.db_path()).map_err(|e| e.to_string())?
    };
    dto::build_session_transcript(&db, &sid).map_err(|e| e.to_string())
}

pub async fn get_session_messages(
    ctx: &CommandContext,
    session_id: Option<String>,
) -> Result<Vec<UiMessage>, String> {
    let sid = match session_id {
        Some(id) => id,
        None => get_app_status(ctx).await?.session_id,
    };
    let db = {
        let cfg = ctx.config.read().await;
        novel_state::Database::open(cfg.db_path()).map_err(|e| e.to_string())?
    };
    let stored = db
        .get_session_messages(&sid, None)
        .map_err(|e| e.to_string())?;
    Ok(stored_messages_to_ui(&stored))
}

pub async fn list_sessions(
    ctx: &CommandContext,
) -> Result<Vec<novel_state::SessionSummary>, String> {
    let (db, root) = {
        let cfg = ctx.config.read().await;
        (
            novel_state::Database::open(cfg.db_path()).map_err(|e| e.to_string())?,
            cfg.active_project.to_string_lossy().to_string(),
        )
    };
    db.list_sessions(&root, 50).map_err(|e| e.to_string())
}

pub async fn list_project_files(
    ctx: &CommandContext,
) -> Result<Vec<novel_knowledge::ProjectFileEntry>, String> {
    let cfg = ctx.config.read().await;
    novel_knowledge::list_project_files(&cfg.active_project).map_err(|e| e.to_string())
}

pub async fn read_project_file(ctx: &CommandContext, path: String) -> Result<String, String> {
    let cfg = ctx.config.read().await;
    novel_knowledge::read_project_file(&cfg.active_project, &path).map_err(|e| e.to_string())
}

pub async fn update_session_todo(
    ctx: &CommandContext,
    todo_id: String,
    status: String,
) -> Result<(), String> {
    let session_id = get_app_status(ctx).await?.session_id;
    let db = {
        let cfg = ctx.config.read().await;
        novel_state::Database::open(cfg.db_path()).map_err(|e| e.to_string())?
    };
    let todos = db
        .list_session_todos(&session_id)
        .map_err(|e| e.to_string())?;
    let Some(existing) = todos.iter().find(|t| t.id == todo_id) else {
        return Err(format!("todo not found: {todo_id}"));
    };
    db.upsert_session_todos(
        &session_id,
        &[novel_state::SessionTodo {
            id: todo_id,
            content: existing.content.clone(),
            status,
        }],
        true,
    )
    .map_err(|e| e.to_string())
}

pub async fn get_api_config(ctx: &CommandContext) -> Result<novel_config::AgentApiConfig, String> {
    let path = ctx.config.read().await.global_config_path.clone();
    let cfg = novel_config::load_agent_api_config(&path)
        .map_err(|e| e.to_string())?
        .unwrap_or_default();
    if cfg.api_key.is_empty() {
        return Err("no api config set".into());
    }
    Ok(novel_config::AgentApiConfig {
        api_key: "••••••••".to_string(),
        api_base: cfg.api_base,
    })
}

pub async fn set_api_config(
    ctx: &CommandContext,
    api_key: String,
    api_base: String,
) -> Result<(), String> {
    let path = ctx.config.read().await.global_config_path.clone();
    novel_config::save_agent_api_config(&path, &novel_config::AgentApiConfig { api_key, api_base })
        .map_err(|e| e.to_string())
}

#[cfg(test)]
mod tests {
    #[test]
    fn parse_agent_type_accepts_general_purpose() {
        assert_eq!(
            novel_core::AgentType::parse("GeneralPurpose"),
            Some(novel_core::AgentType::GeneralPurpose)
        );
        assert_eq!(
            novel_core::AgentType::parse("general-purpose"),
            Some(novel_core::AgentType::GeneralPurpose)
        );
    }
}
