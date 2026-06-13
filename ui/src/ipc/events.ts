/** Tauri event names (must match `crates/novel-server/src/tauri/event_payload.rs`). */
export const IPC_EVENTS = {
  streamChunk: "stream-chunk",
  toolCallRequest: "tool-call-request",
  turnComplete: "turn-complete",
  askUserQuestion: "ask-user-question",
  assistantSegmentComplete: "assistant-segment-complete",
  sessionTokensUpdated: "session-tokens-updated",
  sessionTodosUpdated: "session-todos-updated",
  sessionResumed: "session-resumed",
  permissionModeChanged: "permission-mode-changed",
  compactionProgress: "compaction-progress",
  subAgentStarted: "sub-agent-started",
  subAgentStream: "sub-agent-stream",
  subAgentTool: "sub-agent-tool",
  subAgentComplete: "sub-agent-complete",
  interruptibleStatusChanged: "interruptible-status-changed",
} as const;
