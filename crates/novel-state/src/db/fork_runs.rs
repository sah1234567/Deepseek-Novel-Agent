use super::Database;
use crate::StateError;
use chrono::Utc;
use rusqlite::params;
use uuid::Uuid;

impl Database {
    /// Start a sub-agent fork run. Transcript rows go to `fork_messages`, not parent `messages`.
    pub fn create_fork_run(
        &self,
        session_id: &str,
        parent_turn_number: i32,
        agent_type: &str,
        task: &str,
        source: &str,
    ) -> Result<String, StateError> {
        let id = Uuid::new_v4().to_string();
        let conn = self.pool.get()?;
        conn.execute(
            "INSERT INTO fork_runs (id, session_id, parent_turn_number, agent_type, task, source, status, started_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, 'running', ?7)",
            params![
                id,
                session_id,
                parent_turn_number,
                agent_type,
                task,
                source,
                Utc::now().to_rfc3339()
            ],
        )?;
        Ok(id)
    }

    pub fn insert_fork_message(
        &self,
        run_id: &str,
        role: &str,
        content: &serde_json::Value,
    ) -> Result<(String, i32), StateError> {
        let conn = self.pool.get()?;
        let sequence: i32 = conn.query_row(
            "SELECT COALESCE(MAX(sequence), -1) + 1 FROM fork_messages WHERE run_id = ?1",
            params![run_id],
            |row| row.get(0),
        )?;
        let id = Uuid::new_v4().to_string();
        conn.execute(
            "INSERT INTO fork_messages (id, run_id, sequence, role, content_json, created_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            params![
                id,
                run_id,
                sequence,
                role,
                content.to_string(),
                Utc::now().to_rfc3339()
            ],
        )?;
        Ok((id, sequence))
    }

    pub fn finish_fork_run(
        &self,
        run_id: &str,
        status: &str,
        report_message_id: Option<&str>,
    ) -> Result<(), StateError> {
        let conn = self.pool.get()?;
        let n = conn.execute(
            "UPDATE fork_runs SET status = ?1, report_message_id = ?2, finished_at = ?3 WHERE id = ?4",
            params![
                status,
                report_message_id,
                Utc::now().to_rfc3339(),
                run_id
            ],
        )?;
        if n == 0 {
            return Err(StateError::ForkRunNotFound(run_id.into()));
        }
        Ok(())
    }

    pub fn get_fork_messages(&self, run_id: &str) -> Result<Vec<crate::ForkMessage>, StateError> {
        let conn = self.pool.get()?;
        let mut stmt = conn.prepare(
            "SELECT id, run_id, sequence, role, content_json, created_at
             FROM fork_messages WHERE run_id = ?1 ORDER BY sequence",
        )?;
        let mut rows = stmt.query(params![run_id])?;
        let mut out = Vec::new();
        while let Some(row) = rows.next()? {
            let created: String = row.get(5)?;
            let content_str: String = row.get(4)?;
            let content_json: serde_json::Value = serde_json::from_str(&content_str)?;
            out.push(crate::ForkMessage {
                id: row.get(0)?,
                run_id: row.get(1)?,
                sequence: row.get(2)?,
                role: row.get(3)?,
                content_json,
                created_at: created.parse().unwrap_or_else(|_| Utc::now()),
            });
        }
        Ok(out)
    }
}
