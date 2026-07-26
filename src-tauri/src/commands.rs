use novel_server::tauri::AppState;
use tauri::{AppHandle, State};

#[tauri::command]
pub async fn send_message(
    state: State<'_, AppState>,
    app: AppHandle,
    content: String,
    model: Option<String>,
) -> Result<String, String> {
    let ctx = state.command_context(app);
    novel_server::tauri::send_message(&ctx, content, model).await
}

#[tauri::command]
pub async fn interrupt(
    state: State<'_, AppState>,
    app: AppHandle,
    reason: Option<String>,
) -> Result<(), String> {
    let ctx = state.command_context(app);
    novel_server::tauri::interrupt(&ctx, reason).await
}

#[tauri::command]
pub async fn approve_tool(
    state: State<'_, AppState>,
    app: AppHandle,
    tool_call_id: String,
) -> Result<(), String> {
    let ctx = state.command_context(app);
    novel_server::tauri::approve_tool(&ctx, tool_call_id).await
}

#[tauri::command]
pub async fn deny_tool(
    state: State<'_, AppState>,
    app: AppHandle,
    tool_call_id: String,
    reason: Option<String>,
) -> Result<(), String> {
    let ctx = state.command_context(app);
    novel_server::tauri::deny_tool(&ctx, tool_call_id, reason).await
}

#[tauri::command]
pub async fn answer_question(
    state: State<'_, AppState>,
    app: AppHandle,
    tool_call_id: String,
    answers: serde_json::Value,
) -> Result<(), String> {
    let ctx = state.command_context(app);
    novel_server::tauri::answer_question(&ctx, tool_call_id, answers).await
}

#[tauri::command]
pub async fn get_app_status(
    state: State<'_, AppState>,
    app: AppHandle,
) -> Result<novel_server::tauri::AppStatus, String> {
    let ctx = state.command_context(app);
    novel_server::tauri::get_app_status(&ctx).await
}

#[tauri::command]
pub async fn set_permission_mode(
    state: State<'_, AppState>,
    app: AppHandle,
    mode: String,
) -> Result<(), String> {
    let ctx = state.command_context(app);
    novel_server::tauri::set_permission_mode(&ctx, mode).await
}

#[tauri::command]
pub async fn set_interaction_mode(
    state: State<'_, AppState>,
    app: AppHandle,
    mode: String,
) -> Result<(), String> {
    let ctx = state.command_context(app);
    novel_server::tauri::set_interaction_mode(&ctx, mode).await
}

#[tauri::command]
pub async fn init_novel_project(state: State<'_, AppState>, app: AppHandle) -> Result<(), String> {
    let ctx = state.command_context(app);
    novel_server::tauri::init_novel_project(&ctx).await
}

#[tauri::command]
pub async fn create_session(state: State<'_, AppState>, app: AppHandle) -> Result<String, String> {
    let ctx = state.command_context(app);
    novel_server::tauri::create_session(&ctx).await
}

#[tauri::command]
pub async fn create_work(
    state: State<'_, AppState>,
    app: AppHandle,
    name: String,
) -> Result<String, String> {
    let ctx = state.command_context(app);
    novel_server::tauri::create_work(&ctx, name).await
}

#[tauri::command]
pub async fn open_work(
    state: State<'_, AppState>,
    app: AppHandle,
    name: String,
) -> Result<String, String> {
    let ctx = state.command_context(app);
    novel_server::tauri::open_work(&ctx, name).await
}

#[tauri::command]
pub async fn list_works(
    state: State<'_, AppState>,
    app: AppHandle,
) -> Result<Vec<novel_server::tauri::WorkSummary>, String> {
    let ctx = state.command_context(app);
    novel_server::tauri::list_works_cmd(&ctx).await
}

#[tauri::command]
pub async fn resume_session(
    state: State<'_, AppState>,
    app: AppHandle,
    session_id: String,
) -> Result<String, String> {
    let ctx = state.command_context(app);
    novel_server::tauri::resume_session(&ctx, session_id).await
}

#[tauri::command]
pub async fn get_session_transcript_layout(
    state: State<'_, AppState>,
    app: AppHandle,
    session_id: Option<String>,
) -> Result<novel_server::tauri::SessionTranscriptLayout, String> {
    let ctx = state.command_context(app);
    novel_server::tauri::get_session_transcript_layout(&ctx, session_id).await
}

#[tauri::command]
pub async fn get_session_message_turns(
    state: State<'_, AppState>,
    app: AppHandle,
    session_id: Option<String>,
    from_turn: i32,
    to_turn: i32,
) -> Result<Vec<novel_server::tauri::UiTurnBundle>, String> {
    let ctx = state.command_context(app);
    novel_server::tauri::get_session_message_turns(&ctx, session_id, from_turn, to_turn).await
}

#[tauri::command]
pub async fn get_session_archive_turns(
    state: State<'_, AppState>,
    app: AppHandle,
    session_id: Option<String>,
    epoch: i32,
    from_turn: i32,
    to_turn: i32,
) -> Result<Vec<novel_server::tauri::UiTurnBundle>, String> {
    let ctx = state.command_context(app);
    novel_server::tauri::get_session_archive_turns(&ctx, session_id, epoch, from_turn, to_turn)
        .await
}

#[tauri::command]
pub async fn list_sessions(
    state: State<'_, AppState>,
    app: AppHandle,
) -> Result<Vec<novel_state::SessionSummary>, String> {
    let ctx = state.command_context(app);
    novel_server::tauri::list_sessions(&ctx).await
}

#[tauri::command]
pub async fn list_project_files(
    state: State<'_, AppState>,
    app: AppHandle,
) -> Result<Vec<novel_knowledge::ProjectFileEntry>, String> {
    let ctx = state.command_context(app);
    novel_server::tauri::list_project_files(&ctx).await
}

#[tauri::command]
pub async fn read_project_file(
    state: State<'_, AppState>,
    app: AppHandle,
    path: String,
) -> Result<String, String> {
    let ctx = state.command_context(app);
    novel_server::tauri::read_project_file(&ctx, path).await
}

#[tauri::command]
pub async fn update_session_todo(
    state: State<'_, AppState>,
    app: AppHandle,
    session_id: String,
    todo_id: String,
    status: String,
) -> Result<(), String> {
    let ctx = state.command_context(app);
    novel_server::tauri::update_session_todo(&ctx, session_id, todo_id, status).await
}

#[tauri::command]
pub async fn get_api_config(
    state: State<'_, AppState>,
    app: AppHandle,
) -> Result<novel_config::AgentApiConfig, String> {
    let ctx = state.command_context(app);
    novel_server::tauri::get_api_config(&ctx).await
}

#[tauri::command]
pub async fn get_fork_messages(
    state: State<'_, AppState>,
    app: AppHandle,
    run_id: String,
) -> Result<Vec<novel_server::tauri::UiMessage>, String> {
    let ctx = state.command_context(app);
    novel_server::tauri::get_fork_messages(&ctx, run_id).await
}

#[tauri::command]
pub fn subscribe_fork_stream(
    state: State<'_, AppState>,
    app: AppHandle,
    run_id: String,
) -> Result<(), String> {
    let ctx = state.command_context(app);
    novel_server::tauri::subscribe_fork_stream(&ctx, run_id)
}

#[tauri::command]
pub fn unsubscribe_fork_stream(
    state: State<'_, AppState>,
    app: AppHandle,
    run_id: String,
) -> Result<(), String> {
    let ctx = state.command_context(app);
    novel_server::tauri::unsubscribe_fork_stream(&ctx, run_id)
}

#[tauri::command]
pub async fn set_api_config(
    state: State<'_, AppState>,
    app: AppHandle,
    api_key: String,
    api_base: String,
) -> Result<(), String> {
    let ctx = state.command_context(app);
    novel_server::tauri::set_api_config(&ctx, api_key, api_base).await
}

#[tauri::command]
pub async fn graph_get_state(
    state: State<'_, AppState>,
    app: AppHandle,
) -> Result<novel_graph::GraphStateSnapshot, String> {
    let ctx = state.command_context(app);
    novel_server::tauri::graph_get_state(&ctx).await
}

#[tauri::command]
pub async fn graph_get_node(
    state: State<'_, AppState>,
    app: AppHandle,
    node_id: String,
) -> Result<serde_json::Value, String> {
    let ctx = state.command_context(app);
    novel_server::tauri::graph_get_node(&ctx, node_id).await
}

#[tauri::command]
pub async fn graph_get_loop(
    state: State<'_, AppState>,
    app: AppHandle,
    loop_id: String,
) -> Result<serde_json::Value, String> {
    let ctx = state.command_context(app);
    novel_server::tauri::graph_get_loop(&ctx, loop_id).await
}

#[tauri::command]
pub async fn graph_activate_node(
    state: State<'_, AppState>,
    app: AppHandle,
    node_id: String,
) -> Result<(), String> {
    let ctx = state.command_context(app);
    novel_server::tauri::graph_activate_node(&ctx, node_id).await
}

#[tauri::command]
pub async fn graph_clear_focus(state: State<'_, AppState>, app: AppHandle) -> Result<(), String> {
    let ctx = state.command_context(app);
    novel_server::tauri::graph_clear_focus(&ctx).await
}

#[tauri::command]
pub async fn graph_start_node(
    state: State<'_, AppState>,
    app: AppHandle,
    node_id: String,
) -> Result<(), String> {
    let ctx = state.command_context(app);
    novel_server::tauri::graph_start_node(&ctx, node_id).await
}

#[tauri::command]
pub async fn graph_approve(
    state: State<'_, AppState>,
    app: AppHandle,
    node_id: String,
) -> Result<novel_server::tauri::GraphMutateResult, String> {
    let ctx = state.command_context(app);
    novel_server::tauri::graph_approve(&ctx, node_id).await
}

#[tauri::command]
pub async fn graph_reject(
    state: State<'_, AppState>,
    app: AppHandle,
    node_id: String,
    note: String,
) -> Result<novel_server::tauri::GraphMutateResult, String> {
    let ctx = state.command_context(app);
    novel_server::tauri::graph_reject(&ctx, node_id, note).await
}

#[tauri::command]
pub async fn graph_reopen(
    state: State<'_, AppState>,
    app: AppHandle,
    node_id: String,
    cascade_downstream: Option<bool>,
) -> Result<novel_server::tauri::GraphMutateResult, String> {
    let ctx = state.command_context(app);
    novel_server::tauri::graph_reopen(&ctx, node_id, cascade_downstream).await
}

#[tauri::command]
pub async fn graph_loop_pause(
    state: State<'_, AppState>,
    app: AppHandle,
    loop_id: String,
    reason: Option<String>,
) -> Result<novel_server::tauri::GraphMutateResult, String> {
    let ctx = state.command_context(app);
    novel_server::tauri::graph_loop_pause(&ctx, loop_id, reason).await
}

#[tauri::command]
pub async fn graph_loop_resume(
    state: State<'_, AppState>,
    app: AppHandle,
    loop_id: String,
) -> Result<novel_server::tauri::GraphMutateResult, String> {
    let ctx = state.command_context(app);
    novel_server::tauri::graph_loop_resume(&ctx, loop_id).await
}

#[tauri::command]
pub async fn graph_loop_set_target(
    state: State<'_, AppState>,
    app: AppHandle,
    loop_id: String,
    key: String,
    value: serde_json::Value,
) -> Result<novel_server::tauri::GraphMutateResult, String> {
    let ctx = state.command_context(app);
    novel_server::tauri::graph_loop_set_target(&ctx, loop_id, key, value).await
}

#[tauri::command]
pub async fn graph_loop_set_cursor(
    state: State<'_, AppState>,
    app: AppHandle,
    loop_id: String,
    counters: std::collections::HashMap<String, i64>,
) -> Result<novel_server::tauri::GraphMutateResult, String> {
    let ctx = state.command_context(app);
    novel_server::tauri::graph_loop_set_cursor(&ctx, loop_id, counters).await
}

#[tauri::command]
pub async fn graph_loop_list_history(
    state: State<'_, AppState>,
    app: AppHandle,
    loop_id: String,
    limit: Option<u32>,
) -> Result<Vec<novel_server::tauri::LoopHistoryRow>, String> {
    let ctx = state.command_context(app);
    novel_server::tauri::graph_loop_list_history(&ctx, loop_id, limit).await
}

#[tauri::command]
pub async fn graph_preview_template(
    state: State<'_, AppState>,
    app: AppHandle,
) -> Result<String, String> {
    let ctx = state.command_context(app);
    novel_server::tauri::graph_preview_template(&ctx).await
}

#[tauri::command]
pub async fn graph_apply_template(
    state: State<'_, AppState>,
    app: AppHandle,
    force: Option<bool>,
) -> Result<novel_graph::GraphStateSnapshot, String> {
    let ctx = state.command_context(app);
    novel_server::tauri::graph_apply_template(&ctx, force).await
}
