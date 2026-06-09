use serde::{Deserialize, Serialize};
use serde_json::Value;

/// Same enum as `novel_deepseek::StreamEvent` deltas; re-exported for `Event` / Tauri IPC.
pub use novel_deepseek::ContentBlockKind;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ToolCallRecord {
    pub id: String,
    pub name: String,
    pub arguments: Value,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default)]
pub struct ChatMessage {
    pub role: String,
    pub content: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tool_call_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tool_calls: Option<Vec<ToolCallRecord>>,
    /// Thinking/CoT content. Persisted to DB for frontend display,
    /// but stripped before sending back to the API.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reasoning_content: Option<String>,
}

#[derive(Debug, Clone)]
pub enum Op {
    SendMessage {
        content: String,
        model: Option<String>,
    },
    Interrupt,
    ApproveTool {
        tool_call_id: String,
    },
    DenyTool {
        tool_call_id: String,
        reason: Option<String>,
    },
    ResumeSession {
        session_id: String,
    },
}

#[derive(Debug, Clone)]
pub enum Event {
    ContentBlockDelta {
        message_id: String,
        index: u32,
        delta: String,
        kind: ContentBlockKind,
    },
    ToolCallRequest {
        tool_call_id: String,
        name: String,
        input: Value,
        needs_approval: bool,
    },
    /// Tool block appeared in the stream (before arguments are complete).
    ToolUseStarted {
        tool_call_id: String,
        name: String,
    },
    /// Partial tool arguments JSON while streaming.
    ToolInputDelta {
        tool_call_id: String,
        delta: String,
    },
    /// Arguments JSON complete during stream; parsed input for UI + early approval.
    ToolInputComplete {
        tool_call_id: String,
        name: String,
        input: Value,
        needs_approval: bool,
    },
    ToolCallProgress {
        tool_call_id: String,
        status: String,
        description: String,
    },
    ToolCallResult {
        tool_call_id: String,
        content: String,
    },
    AskUserQuestion {
        tool_call_id: String,
        payload: novel_tools::AskUserQuestionPayload,
    },
    TurnStart {
        turn_number: u32,
    },
    TurnComplete {
        turn_number: u32,
        cache_hit_tokens: i64,
        cache_miss_tokens: i64,
        completion_tokens: i64,
        turn_hit_tokens: i64,
        turn_miss_tokens: i64,
        turn_comp_tokens: i64,
        was_interrupted: bool,
    },
    /// Main-session interruptible tool flag changed (replaces 500ms `get_app_status` polling).
    InterruptibleStatusChanged {
        has_interruptible_tool_in_progress: bool,
    },
    /// Session token counters updated after each LLM API call (main and subagent; StatusBar live refresh).
    SessionTokensUpdated {
        cache_hit_tokens: i64,
        cache_miss_tokens: i64,
        completion_tokens: i64,
        context_tokens: i64,
    },
    /// Sub-agent lifecycle + scoped stream/tool events. `fork_run_id` keys overlay state; never merged into parent LLM messages.
    SubAgentStarted {
        fork_run_id: String,
        agent_id: String,
        agent_type: String,
        task_preview: String,
        /// Main-session `ForkSubAgent` tool_call id; `None` for PostToolUse hook path.
        parent_tool_call_id: Option<String>,
    },
    SubAgentComplete {
        fork_run_id: String,
        agent_id: String,
        output: String,
        cache_hit_rate: f64,
    },
    /// Scoped LLM stream delta for sub-agent overlay (not main chat).
    SubAgentStreamDelta {
        fork_run_id: String,
        delta: String,
        kind: ContentBlockKind,
    },
    /// Scoped tool lifecycle for sub-agent overlay.
    SubAgentToolUpdate {
        fork_run_id: String,
        phase: String,
        tool_call_id: String,
        tool_name: Option<String>,
        input: Option<Value>,
        content: Option<String>,
        needs_approval: Option<bool>,
        status: Option<String>,
        description: Option<String>,
    },
    CompactionProgress {
        attempt: u32,
        action: CompactionAction,
    },
    Error {
        message: String,
        recoverable: bool,
    },
    /// LLM stream finished for one inner-turn iteration; UI should freeze CoT/text and start a new box on the next stream.
    AssistantSegmentComplete {
        segment_index: u32,
        fork_run_id: Option<String>,
    },
}

#[derive(Debug, Clone)]
pub enum CompactionAction {
    Started,
    GeneratingSummary,
    RebuildingSession,
    Done {
        tokens_before: usize,
        tokens_after: usize,
        epoch: i32,
        retained_min_turn: Option<i32>,
        retained_max_turn: Option<i32>,
    },
    Failed {
        reason: String,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TerminalReason {
    Completed,
    MaxReactLoops(u32),
    /// Same tool error repeated too many times in one turn (death-spiral guard).
    RepeatedToolFailures {
        tool: String,
        detail: String,
    },
    AbortedStreaming,
    AbortedTools,
    ModelError {
        message: String,
    },
}

impl TerminalReason {
    pub fn is_aborted(&self) -> bool {
        matches!(self, Self::AbortedStreaming | Self::AbortedTools)
    }
}
