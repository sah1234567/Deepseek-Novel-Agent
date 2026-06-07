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
    todo_id: String,
    status: String,
) -> Result<(), String> {
    let ctx = state.command_context(app);
    novel_server::tauri::update_session_todo(&ctx, todo_id, status).await
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
pub async fn set_api_config(
    state: State<'_, AppState>,
    app: AppHandle,
    api_key: String,
    api_base: String,
) -> Result<(), String> {
    let ctx = state.command_context(app);
    novel_server::tauri::set_api_config(&ctx, api_key, api_base).await
}
