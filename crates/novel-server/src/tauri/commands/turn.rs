use crate::tauri::engine_loop::EngineCommand;
use crate::tauri::events::emit_core_event;
use crate::tauri::state::CommandContext;
use tokio::sync::mpsc;

use super::engine_ipc::send_engine_reply;

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

    let _ = send_engine_reply(ctx, |reply| EngineCommand::SendMessage {
        content,
        model,
        event_tx: Some(event_tx),
        reply,
    })
    .await?;
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
    send_engine_reply(ctx, |reply| EngineCommand::ApproveTool {
        tool_call_id,
        event_tx: Some(event_tx),
        reply,
    })
    .await
}

pub async fn deny_tool(
    ctx: &CommandContext,
    tool_call_id: String,
    reason: Option<String>,
) -> Result<(), String> {
    let event_tx = spawn_event_forwarder(ctx, None);
    send_engine_reply(ctx, |reply| EngineCommand::DenyTool {
        tool_call_id,
        reason,
        event_tx: Some(event_tx),
        reply,
    })
    .await
}

pub async fn answer_question(
    ctx: &CommandContext,
    tool_call_id: String,
    answers: serde_json::Value,
) -> Result<(), String> {
    let event_tx = spawn_event_forwarder(ctx, None);

    let _ = send_engine_reply(ctx, |reply| EngineCommand::AnswerQuestion {
        tool_call_id,
        answers,
        event_tx: Some(event_tx),
        reply,
    })
    .await?;
    Ok(())
}
