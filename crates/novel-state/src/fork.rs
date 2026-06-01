use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ForkMessage {
    pub id: String,
    pub run_id: String,
    pub sequence: i32,
    pub role: String,
    pub content_json: serde_json::Value,
    pub created_at: DateTime<Utc>,
}
