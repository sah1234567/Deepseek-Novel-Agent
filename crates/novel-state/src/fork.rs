use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ForkRun {
    pub id: String,
    pub session_id: String,
    pub parent_turn_number: i32,
    pub agent_type: String,
    pub task: String,
    pub source: String,
    pub status: String,
    pub report_message_id: Option<String>,
    pub started_at: DateTime<Utc>,
    pub finished_at: Option<DateTime<Utc>>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ForkMessage {
    pub id: String,
    pub run_id: String,
    pub sequence: i32,
    pub role: String,
    pub content_json: serde_json::Value,
    pub created_at: DateTime<Utc>,
}
