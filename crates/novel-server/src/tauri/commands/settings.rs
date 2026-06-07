use crate::tauri::engine_loop::{AppStatus, EngineCommand};
use crate::tauri::state::CommandContext;

use super::engine_ipc::{emit_permission_mode_changed, send_engine_reply};
use super::open_db;

pub async fn get_app_status(ctx: &CommandContext) -> Result<AppStatus, String> {
    send_engine_reply(ctx, |reply| EngineCommand::GetStatus { reply }).await
}

pub async fn set_permission_mode(ctx: &CommandContext, mode: String) -> Result<(), String> {
    use std::sync::atomic::Ordering;
    if ctx.turn_in_progress.load(Ordering::Acquire) {
        return Err("当前轮次进行中，请等待结束或中断后再切换权限模式".into());
    }
    send_engine_reply(ctx, |reply| EngineCommand::SetPermissionMode {
        mode: mode.clone(),
        reply,
    })
    .await?;
    emit_permission_mode_changed(ctx, &mode);
    Ok(())
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

pub async fn update_session_todo(
    ctx: &CommandContext,
    todo_id: String,
    status: String,
) -> Result<(), String> {
    let session_id = get_app_status(ctx).await?.session_id;
    let db = open_db(ctx).await?;
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
