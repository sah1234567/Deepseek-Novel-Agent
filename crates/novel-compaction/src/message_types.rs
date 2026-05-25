use serde::{Deserialize, Serialize};

/// Minimal message shape for compaction (avoids novel-core dependency).
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
pub struct CompactionMessage {
    pub role: String,
    pub content: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tool_call_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tool_calls: Option<Vec<CompactionToolCall>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reasoning_content: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CompactionToolCall {
    pub id: String,
    pub name: String,
    pub arguments: String,
}
