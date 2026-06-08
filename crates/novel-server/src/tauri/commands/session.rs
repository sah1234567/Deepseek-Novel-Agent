use crate::tauri::dto::{
    build_session_transcript_layout, fork_messages_to_ui, SessionTranscriptLayout, UiMessage,
    UiTurnBundle,
};
use crate::tauri::engine_loop::EngineCommand;
use crate::tauri::session_api::{self, TurnMessageSource};
use crate::tauri::state::CommandContext;

use super::engine_ipc::{emit_session_resumed, send_engine_reply};
use super::fork::clear_fork_stream_subscriptions;
use super::open_db;
use super::settings::get_app_status;

pub(crate) async fn switch_project_and_create_session(
    ctx: &CommandContext,
    project_root: std::path::PathBuf,
) -> Result<String, String> {
    let sid = send_engine_reply(ctx, |reply| EngineCommand::SwitchProjectAndCreateSession {
        project_root,
        reply,
    })
    .await?;
    clear_fork_stream_subscriptions(&ctx.fork_stream_subs);
    emit_session_resumed(ctx, &sid);
    Ok(sid)
}

pub async fn create_session(ctx: &CommandContext) -> Result<String, String> {
    let sid = send_engine_reply(ctx, |reply| EngineCommand::CreateSession { reply }).await?;
    clear_fork_stream_subscriptions(&ctx.fork_stream_subs);
    emit_session_resumed(ctx, &sid);
    Ok(sid)
}

pub async fn resume_session(ctx: &CommandContext, session_id: String) -> Result<String, String> {
    let sid = send_engine_reply(ctx, |reply| EngineCommand::ResumeSession {
        session_id: session_id.clone(),
        reply,
    })
    .await?;
    clear_fork_stream_subscriptions(&ctx.fork_stream_subs);
    emit_session_resumed(ctx, &sid);
    Ok(sid)
}

async fn resolve_session_id(
    ctx: &CommandContext,
    session_id: Option<String>,
) -> Result<String, String> {
    match session_id {
        Some(id) => Ok(id),
        None => Ok(get_app_status(ctx).await?.session_id),
    }
}

pub async fn get_session_transcript_layout(
    ctx: &CommandContext,
    session_id: Option<String>,
) -> Result<SessionTranscriptLayout, String> {
    let sid = resolve_session_id(ctx, session_id).await?;
    let db = open_db(ctx).await?;
    let layout = build_session_transcript_layout(&db, &sid).map_err(|e| e.to_string())?;
    tracing::debug!(
        session_id = %sid,
        active_min = layout.active.min_turn,
        active_max = layout.active.max_turn,
        archive_epochs = layout.archives.len(),
        has_context_refresh = layout.has_context_refresh,
        "get_session_transcript_layout"
    );
    Ok(layout)
}

async fn fetch_turn_bundles(
    ctx: &CommandContext,
    session_id: Option<String>,
    from_turn: i32,
    to_turn: i32,
    source: TurnMessageSource,
) -> Result<Vec<UiTurnBundle>, String> {
    let sid = resolve_session_id(ctx, session_id).await?;
    let db = open_db(ctx).await?;
    session_api::bundles_for_turn_range(&db, &sid, from_turn, to_turn, source)
}

pub async fn get_session_message_turns(
    ctx: &CommandContext,
    session_id: Option<String>,
    from_turn: i32,
    to_turn: i32,
) -> Result<Vec<UiTurnBundle>, String> {
    fetch_turn_bundles(
        ctx,
        session_id,
        from_turn,
        to_turn,
        TurnMessageSource::Active,
    )
    .await
}

pub async fn get_session_archive_turns(
    ctx: &CommandContext,
    session_id: Option<String>,
    epoch: i32,
    from_turn: i32,
    to_turn: i32,
) -> Result<Vec<UiTurnBundle>, String> {
    fetch_turn_bundles(
        ctx,
        session_id,
        from_turn,
        to_turn,
        TurnMessageSource::Archive(epoch),
    )
    .await
}

pub async fn list_sessions(
    ctx: &CommandContext,
) -> Result<Vec<novel_state::SessionSummary>, String> {
    let root = {
        let cfg = ctx.config.read().await;
        cfg.active_project.to_string_lossy().to_string()
    };
    let db = open_db(ctx).await?;
    db.list_sessions(&root, 50).map_err(|e| e.to_string())
}

pub async fn get_fork_messages(
    ctx: &CommandContext,
    run_id: String,
) -> Result<Vec<UiMessage>, String> {
    let db = open_db(ctx).await?;
    let stored = db.get_fork_messages(&run_id).map_err(|e| e.to_string())?;
    Ok(fork_messages_to_ui(&stored))
}
