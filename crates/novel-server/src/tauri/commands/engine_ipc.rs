use crate::tauri::engine_loop::EngineCommand;
use crate::tauri::state::CommandContext;
use tauri::Emitter;
use tokio::sync::oneshot;

pub(crate) async fn send_engine_reply<T, F>(ctx: &CommandContext, build: F) -> Result<T, String>
where
    F: FnOnce(oneshot::Sender<Result<T, String>>) -> EngineCommand,
{
    let (reply_tx, reply_rx) = oneshot::channel();
    ctx.cmd_tx
        .send(build(reply_tx))
        .map_err(|_| "engine loop stopped".to_string())?;
    reply_rx
        .await
        .map_err(|_| "engine loop stopped".to_string())?
}

pub(crate) fn emit_session_resumed(ctx: &CommandContext, session_id: &str) {
    if let Err(e) = ctx.app_handle.emit(
        "session-resumed",
        serde_json::json!({ "sessionId": session_id }),
    ) {
        tracing::warn!(session_id, error = %e, "session-resumed emit failed");
    }
}

pub(crate) fn emit_permission_mode_changed(ctx: &CommandContext, mode: &str) {
    if let Err(e) = ctx.app_handle.emit(
        "permission-mode-changed",
        serde_json::json!({ "mode": mode }),
    ) {
        tracing::warn!(mode, error = %e, "permission-mode-changed emit failed");
    }
}
