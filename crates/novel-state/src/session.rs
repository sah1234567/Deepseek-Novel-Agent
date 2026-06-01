use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct SessionSummary {
    pub id: String,
    pub title: Option<String>,
    pub status: String,
    pub model: String,
    pub last_active_at: DateTime<Utc>,
    pub created_at: DateTime<Utc>,
    /// User dialogue rounds (one increment per user message).
    pub total_turns: i64,
    /// LLM API call count (inner loop + sub-agents on this session).
    pub api_call_count: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Session {
    pub id: String,
    pub project_root: String,
    pub title: Option<String>,
    pub status: String,
    pub model: String,
    pub provider: String,
    pub created_at: DateTime<Utc>,
    pub last_active_at: DateTime<Utc>,
    pub cache_hit_tokens: i64,
    pub cache_miss_tokens: i64,
    pub completion_tokens: i64,
    pub context_tokens: i64,
    /// User dialogue rounds (one increment per user message).
    pub total_turns: i64,
    /// LLM API call count (inner loop + sub-agents on this session).
    pub api_call_count: i64,
}
