use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Checkpoint {
    pub id: String,
    pub parent_session_id: String,
    pub fork_point: i32,
    pub shared_prefix_hash: String,
    pub created_at: DateTime<Utc>,
}
