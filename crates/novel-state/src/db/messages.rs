use super::Database;
use crate::{message::StoredMessage, turn_bounds::TurnBounds, StateError};
use chrono::Utc;
use rusqlite::{params, OptionalExtension};
use uuid::Uuid;

impl Database {
    pub fn insert_message(
        &self,
        session_id: &str,
        turn_number: i32,
        sequence: i32,
        role: &str,
        content: &serde_json::Value,
        estimated_tokens: Option<i64>,
    ) -> Result<String, StateError> {
        let id = Uuid::new_v4().to_string();
        let conn = self.pool.get()?;
        conn.execute(
            "INSERT INTO messages (id, session_id, turn_number, sequence, role, content_json, estimated_tokens, created_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
            params![
                id,
                session_id,
                turn_number,
                sequence,
                role,
                content.to_string(),
                estimated_tokens,
                Utc::now().to_rfc3339()
            ],
        )?;
        Ok(id)
    }

    pub fn max_message_sequence_for_turn(
        &self,
        session_id: &str,
        turn_number: i32,
    ) -> Result<i32, StateError> {
        let conn = self.pool.get()?;
        let max: i32 = conn.query_row(
            "SELECT COALESCE(MAX(sequence), 0) FROM messages WHERE session_id = ?1 AND turn_number = ?2",
            params![session_id, turn_number],
            |row| row.get(0),
        )?;
        Ok(max)
    }

    pub fn get_session_messages(
        &self,
        session_id: &str,
        turn_range: Option<(i32, i32)>,
    ) -> Result<Vec<StoredMessage>, StateError> {
        let conn = self.pool.get()?;
        let (sql, p1, p2) = match turn_range {
            Some((start, end)) => (
                "SELECT id, session_id, turn_number, sequence, role, content_json,
                        cache_hit_tokens, cache_miss_tokens, completion_tokens, estimated_tokens, created_at
                 FROM messages WHERE session_id = ?1 AND turn_number BETWEEN ?2 AND ?3
                 ORDER BY turn_number, sequence",
                Some(start),
                Some(end),
            ),
            None => (
                "SELECT id, session_id, turn_number, sequence, role, content_json,
                        cache_hit_tokens, cache_miss_tokens, completion_tokens, estimated_tokens, created_at
                 FROM messages WHERE session_id = ?1 ORDER BY turn_number, sequence",
                None,
                None,
            ),
        };
        let mut stmt = conn.prepare(sql)?;
        let rows = match (p1, p2) {
            (Some(a), Some(b)) => stmt.query(params![session_id, a, b])?,
            _ => stmt.query(params![session_id])?,
        };
        map_messages(rows)
    }

    pub fn replace_session_messages(
        &self,
        session_id: &str,
        messages: &[(i32, i32, &str, &serde_json::Value)],
    ) -> Result<(), StateError> {
        tracing::debug!(
            %session_id,
            message_count = messages.len(),
            "replace_session_messages"
        );
        let conn = self.pool.get()?;
        let tx = conn.unchecked_transaction()?;
        tx.execute(
            "DELETE FROM messages WHERE session_id = ?1",
            params![session_id],
        )?;
        for (turn, seq, role, content) in messages {
            let id = Uuid::new_v4().to_string();
            tx.execute(
                "INSERT INTO messages (id, session_id, turn_number, sequence, role, content_json, created_at)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
                params![
                    id,
                    session_id,
                    turn,
                    seq,
                    role,
                    content.to_string(),
                    Utc::now().to_rfc3339()
                ],
            )?;
        }
        tx.commit()?;
        Ok(())
    }

    /// Copy current working-set rows into `message_archive` before compaction replace.
    pub fn archive_session_messages(
        &self,
        session_id: &str,
        compaction_epoch: i32,
    ) -> Result<(), StateError> {
        tracing::debug!(%session_id, compaction_epoch, "archive_session_messages");
        let conn = self.pool.get()?;
        let tx = conn.unchecked_transaction()?;
        let archived_at = Utc::now().to_rfc3339();
        {
            let mut stmt = tx.prepare(
                "SELECT turn_number, sequence, role, content_json FROM messages
                 WHERE session_id = ?1 ORDER BY turn_number, sequence",
            )?;
            let mut rows = stmt.query(params![session_id])?;
            while let Some(row) = rows.next()? {
                let turn: i32 = row.get(0)?;
                let seq: i32 = row.get(1)?;
                let role: String = row.get(2)?;
                let content: String = row.get(3)?;
                let id = Uuid::new_v4().to_string();
                tx.execute(
                    "INSERT INTO message_archive
                     (id, session_id, compaction_epoch, turn_number, sequence, role, content_json, archived_at)
                     VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
                    params![
                        id,
                        session_id,
                        compaction_epoch,
                        turn,
                        seq,
                        role,
                        content,
                        archived_at
                    ],
                )?;
            }
        }
        tx.commit()?;
        Ok(())
    }

    pub fn get_archived_epochs(&self, session_id: &str) -> Result<Vec<i32>, StateError> {
        let conn = self.pool.get()?;
        let mut stmt = conn.prepare(
            "SELECT DISTINCT compaction_epoch FROM message_archive
             WHERE session_id = ?1 ORDER BY compaction_epoch",
        )?;
        let rows = stmt.query_map(params![session_id], |row| row.get(0))?;
        rows.collect::<Result<Vec<_>, _>>()
            .map_err(StateError::from)
    }

    pub fn get_archived_messages(
        &self,
        session_id: &str,
        compaction_epoch: i32,
    ) -> Result<Vec<StoredMessage>, StateError> {
        self.get_archived_messages_turn_range(session_id, compaction_epoch, None)
    }

    pub fn get_active_turn_bounds(
        &self,
        session_id: &str,
    ) -> Result<Option<TurnBounds>, StateError> {
        let conn = self.pool.get()?;
        let bounds: Option<(i32, i32)> = conn
            .query_row(
                "SELECT MIN(turn_number), MAX(turn_number) FROM messages
                 WHERE session_id = ?1 HAVING COUNT(*) > 0",
                params![session_id],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .optional()
            .map_err(StateError::from)?;
        Ok(bounds.map(|(min_turn, max_turn)| TurnBounds::new(min_turn, max_turn)))
    }

    pub fn get_archived_turn_bounds(
        &self,
        session_id: &str,
        compaction_epoch: i32,
    ) -> Result<Option<TurnBounds>, StateError> {
        let conn = self.pool.get()?;
        let bounds: Option<(i32, i32)> = conn
            .query_row(
                "SELECT MIN(turn_number), MAX(turn_number) FROM message_archive
                 WHERE session_id = ?1 AND compaction_epoch = ?2 HAVING COUNT(*) > 0",
                params![session_id, compaction_epoch],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .optional()
            .map_err(StateError::from)?;
        Ok(bounds.map(|(min_turn, max_turn)| TurnBounds::new(min_turn, max_turn)))
    }

    /// Whether `(turn_number=0, sequence=1)` `[上下文刷新]` user exists in the active working set.
    pub fn has_active_context_refresh(&self, session_id: &str) -> Result<bool, StateError> {
        const PREFIX: &str = "[上下文刷新]";
        let conn = self.pool.get()?;
        let content: Option<String> = conn
            .query_row(
                "SELECT content_json FROM messages
                 WHERE session_id = ?1 AND turn_number = 0 AND sequence = 1 AND role = 'user'",
                params![session_id],
                |row| row.get(0),
            )
            .optional()
            .map_err(StateError::from)?;
        let Some(json) = content else {
            return Ok(false);
        };
        let v: serde_json::Value = serde_json::from_str(&json)?;
        Ok(v.get("content")
            .and_then(|c| c.as_str())
            .is_some_and(|c| c.starts_with(PREFIX)))
    }

    pub fn get_archived_messages_turn_range(
        &self,
        session_id: &str,
        compaction_epoch: i32,
        turn_range: Option<(i32, i32)>,
    ) -> Result<Vec<StoredMessage>, StateError> {
        let conn = self.pool.get()?;
        let (sql, p3, p4) = match turn_range {
            Some((start, end)) => (
                "SELECT id, session_id, turn_number, sequence, role, content_json,
                        0, 0, 0, NULL, archived_at
                 FROM message_archive
                 WHERE session_id = ?1 AND compaction_epoch = ?2
                   AND turn_number BETWEEN ?3 AND ?4
                 ORDER BY turn_number, sequence",
                Some(start),
                Some(end),
            ),
            None => (
                "SELECT id, session_id, turn_number, sequence, role, content_json,
                        0, 0, 0, NULL, archived_at
                 FROM message_archive
                 WHERE session_id = ?1 AND compaction_epoch = ?2
                 ORDER BY turn_number, sequence",
                None,
                None,
            ),
        };
        let mut stmt = conn.prepare(sql)?;
        let rows = match (p3, p4) {
            (Some(a), Some(b)) => stmt.query(params![session_id, compaction_epoch, a, b])?,
            _ => stmt.query(params![session_id, compaction_epoch])?,
        };
        map_messages(rows)
    }
}

fn map_messages(mut rows: rusqlite::Rows<'_>) -> Result<Vec<StoredMessage>, StateError> {
    let mut out = Vec::new();
    while let Some(row) = rows.next()? {
        let content_str: String = row.get(5)?;
        let created: String = row.get(10)?;
        out.push(StoredMessage {
            id: row.get(0)?,
            session_id: row.get(1)?,
            turn_number: row.get(2)?,
            sequence: row.get(3)?,
            role: row.get(4)?,
            content_json: serde_json::from_str(&content_str)?,
            cache_hit_tokens: row.get(6)?,
            cache_miss_tokens: row.get(7)?,
            completion_tokens: row.get(8)?,
            estimated_tokens: row.get(9)?,
            created_at: created.parse().unwrap_or_else(|_| Utc::now()),
        });
    }
    Ok(out)
}
