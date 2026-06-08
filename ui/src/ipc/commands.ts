/**
 * Tauri invoke command names (must match `src-tauri/src/commands.rs` / `novel-server` commands).
 * Args use camelCase in TS; Rust command params are snake_case (serde rename).
 */
export const IPC_COMMANDS = {
  sendMessage: "send_message",
  interrupt: "interrupt",
  approveTool: "approve_tool",
  denyTool: "deny_tool",
  answerQuestion: "answer_question",
  getAppStatus: "get_app_status",
  setPermissionMode: "set_permission_mode",
  initNovelProject: "init_novel_project",
  createSession: "create_session",
  resumeSession: "resume_session",
  createWork: "create_work",
  openWork: "open_work",
  listWorks: "list_works",
  listSessions: "list_sessions",
  listProjectFiles: "list_project_files",
  readProjectFile: "read_project_file",
  updateSessionTodo: "update_session_todo",
  getApiConfig: "get_api_config",
  setApiConfig: "set_api_config",
  getForkMessages: "get_fork_messages",
  subscribeForkStream: "subscribe_fork_stream",
  unsubscribeForkStream: "unsubscribe_fork_stream",
  getSessionTranscriptLayout: "get_session_transcript_layout",
  getSessionMessageTurns: "get_session_message_turns",
  getSessionArchiveTurns: "get_session_archive_turns",
} as const;
