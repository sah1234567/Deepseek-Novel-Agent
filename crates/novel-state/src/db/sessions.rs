use super::Database;
use crate::{session::Session, StateError};
use chrono::Utc;
use rusqlite::{params, TransactionBehavior};
use uuid::Uuid;

impl Database {
    pub fn list_tables(&self) -> Result<Vec<String>, StateError> {
        let conn = self.pool.get()?;
        let mut stmt =
            conn.prepare("SELECT name FROM sqlite_master WHERE type='table' ORDER BY name")?;
        let rows = stmt.query_map([], |row| row.get(0))?;
        Ok(rows.filter_map(|r| r.ok()).collect())
    }

    pub fn session_count(&self) -> Result<i64, StateError> {
        let conn = self.pool.get()?;
        let count: i64 = conn.query_row("SELECT COUNT(*) FROM sessions", [], |r| r.get(0))?;
        Ok(count)
    }

    pub fn create_session(&self, project_root: &str, model: &str) -> Result<String, StateError> {
        let id = Uuid::new_v4().to_string();
        let now = Utc::now().to_rfc3339();
        let conn = self.pool.get()?;
        conn.execute(
            "INSERT INTO sessions (id, project_root, status, model, provider, created_at, last_active_at)
             VALUES (?1, ?2, 'active', ?3, 'deepseek', ?4, ?4)",
            params![id, project_root, model, now],
        )?;
        Ok(id)
    }

    pub fn list_sessions(
        &self,
        project_root: &str,
        limit: i32,
    ) -> Result<Vec<crate::SessionSummary>, StateError> {
        let conn = self.pool.get()?;
        let mut stmt = conn.prepare(
            "SELECT id, title, status, model, last_active_at, created_at, total_turns, api_call_count
             FROM sessions WHERE project_root = ?1
             ORDER BY last_active_at DESC LIMIT ?2",
        )?;
        let rows = stmt.query_map(params![project_root, limit], |row| {
            let last_active: String = row.get(4)?;
            let created: String = row.get(5)?;
            Ok(crate::SessionSummary {
                id: row.get(0)?,
                title: row.get(1)?,
                status: row.get(2)?,
                model: row.get(3)?,
                last_active_at: last_active.parse().unwrap_or_else(|_| Utc::now()),
                created_at: created.parse().unwrap_or_else(|_| Utc::now()),
                total_turns: row.get(6)?,
                api_call_count: row.get(7)?,
            })
        })?;
        Ok(rows.filter_map(|r| r.ok()).collect())
    }

    pub fn get_session(&self, id: &str) -> Result<Option<Session>, StateError> {
        let conn = self.pool.get()?;
        let mut stmt = conn.prepare(
            "SELECT id, project_root, title, status, model, provider, created_at, last_active_at,
                    cache_hit_tokens, cache_miss_tokens, completion_tokens, context_tokens,
                    total_turns, api_call_count
             FROM sessions WHERE id = ?1",
        )?;
        let mut rows = stmt.query(params![id])?;
        if let Some(row) = rows.next()? {
            Ok(Some(row_to_session(row)?))
        } else {
            Ok(None)
        }
    }

    pub fn update_session_status(&self, id: &str, status: &str) -> Result<(), StateError> {
        let conn = self.pool.get()?;
        let n = conn.execute(
            "UPDATE sessions SET status = ?1 WHERE id = ?2",
            params![status, id],
        )?;
        if n == 0 {
            return Err(StateError::SessionNotFound(id.into()));
        }
        Ok(())
    }

    /// Mark session activity at turn/API boundary (does not change counters).
    pub fn touch_last_active_at(&self, session_id: &str) -> Result<(), StateError> {
        let conn = self.pool.get()?;
        let n = conn.execute(
            "UPDATE sessions SET last_active_at = ?1 WHERE id = ?2",
            params![Utc::now().to_rfc3339(), session_id],
        )?;
        if n == 0 {
            return Err(StateError::SessionNotFound(session_id.into()));
        }
        Ok(())
    }

    /// Accumulate session token counters and LLM API call count (billing total).
    ///
    /// When `update_context_snapshot` is true, `context_tokens` is overwritten with this call's
    /// `hit + miss + completion` (main Agent working-set snapshot for StatusBar). SubAgent billing
    /// calls pass false so the parent snapshot is preserved.
    pub fn accumulate_session_tokens(
        &self,
        session_id: &str,
        hit: i64,
        miss: i64,
        completion: i64,
        last_model: &str,
        update_context_snapshot: bool,
    ) -> Result<(), StateError> {
        let mut conn = self.pool.get()?;
        let tx = conn.transaction_with_behavior(TransactionBehavior::Immediate)?;
        let sql = if update_context_snapshot {
            "UPDATE sessions SET
                cache_hit_tokens = cache_hit_tokens + ?1,
                cache_miss_tokens = cache_miss_tokens + ?2,
                completion_tokens = completion_tokens + ?3,
                context_tokens = ?1 + ?2 + ?3,
                api_call_count = api_call_count + 1,
                last_active_at = ?4,
                model = ?5
             WHERE id = ?6"
        } else {
            "UPDATE sessions SET
                cache_hit_tokens = cache_hit_tokens + ?1,
                cache_miss_tokens = cache_miss_tokens + ?2,
                completion_tokens = completion_tokens + ?3,
                api_call_count = api_call_count + 1,
                last_active_at = ?4,
                model = ?5
             WHERE id = ?6"
        };
        let n = tx.execute(
            sql,
            params![
                hit,
                miss,
                completion,
                Utc::now().to_rfc3339(),
                last_model,
                session_id
            ],
        )?;
        if n == 0 {
            return Err(StateError::SessionNotFound(session_id.into()));
        }
        tx.commit()?;
        Ok(())
    }

    /// Persist user dialogue round count (one per user message / `turn_number`).
    pub fn sync_user_turn_count(
        &self,
        session_id: &str,
        user_turns: i32,
    ) -> Result<(), StateError> {
        let conn = self.pool.get()?;
        let n = conn.execute(
            "UPDATE sessions SET total_turns = ?1 WHERE id = ?2",
            params![user_turns, session_id],
        )?;
        if n == 0 {
            return Err(StateError::SessionNotFound(session_id.into()));
        }
        Ok(())
    }

    pub fn set_session_title(&self, session_id: &str, title: &str) -> Result<(), StateError> {
        let conn = self.pool.get()?;
        conn.execute(
            "UPDATE sessions SET title = ?1 WHERE id = ?2 AND title IS NULL",
            params![title, session_id],
        )?;
        Ok(())
    }
}

fn row_to_session(row: &rusqlite::Row<'_>) -> Result<Session, rusqlite::Error> {
    let created: String = row.get(6)?;
    let last: String = row.get(7)?;
    Ok(Session {
        id: row.get(0)?,
        project_root: row.get(1)?,
        title: row.get(2)?,
        status: row.get(3)?,
        model: row.get(4)?,
        provider: row.get(5)?,
        created_at: created.parse().unwrap_or_else(|_| Utc::now()),
        last_active_at: last.parse().unwrap_or_else(|_| Utc::now()),
        cache_hit_tokens: row.get(8)?,
        cache_miss_tokens: row.get(9)?,
        completion_tokens: row.get(10)?,
        context_tokens: row.get(11)?,
        total_turns: row.get(12)?,
        api_call_count: row.get(13)?,
    })
}
