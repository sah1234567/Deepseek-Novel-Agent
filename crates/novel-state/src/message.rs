use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StoredMessage {
    pub id: String,
    pub session_id: String,
    pub turn_number: i32,
    pub sequence: i32,
    pub role: String,
    pub content_json: serde_json::Value,
    pub cache_hit_tokens: i64,
    pub cache_miss_tokens: i64,
    pub completion_tokens: i64,
    pub estimated_tokens: Option<i64>,
    pub created_at: DateTime<Utc>,
}
