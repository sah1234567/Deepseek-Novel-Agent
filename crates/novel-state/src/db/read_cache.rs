use super::Database;
use crate::StateError;
use chrono::Utc;
use rusqlite::params;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReadCacheAnchor {
    pub compaction_count: i32,
    pub anchor_turn: i32,
    pub anchor_sequence: i32,
}

impl Database {
    pub fn list_session_read_cache(
        &self,
        session_id: &str,
    ) -> Result<Vec<(String, String)>, StateError> {
        let conn = self.pool.get()?;
        let mut stmt = conn.prepare(
            "SELECT path, entry_json FROM session_read_cache WHERE session_id = ?1 ORDER BY path",
        )?;
        let rows = stmt.query_map(params![session_id], |row| {
            Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
        })?;
        rows.collect::<Result<Vec<_>, _>>()
            .map_err(StateError::from)
    }

    pub fn upsert_session_read_cache_entry(
        &self,
        session_id: &str,
        path: &str,
        entry_json: &str,
    ) -> Result<(), StateError> {
        let conn = self.pool.get()?;
        let now = Utc::now().to_rfc3339();
        conn.execute(
            "INSERT INTO session_read_cache (session_id, path, entry_json, updated_at)
             VALUES (?1, ?2, ?3, ?4)
             ON CONFLICT(session_id, path) DO UPDATE SET
               entry_json = excluded.entry_json,
               updated_at = excluded.updated_at",
            params![session_id, path, entry_json, now],
        )?;
        Ok(())
    }

    pub fn delete_session_read_cache_paths_not_in(
        &self,
        session_id: &str,
        keep_paths: &[String],
    ) -> Result<(), StateError> {
        let conn = self.pool.get()?;
        if keep_paths.is_empty() {
            conn.execute(
                "DELETE FROM session_read_cache WHERE session_id = ?1",
                params![session_id],
            )?;
            return Ok(());
        }
        let placeholders = keep_paths
            .iter()
            .enumerate()
            .map(|(i, _)| format!("?{}", i + 2))
            .collect::<Vec<_>>()
            .join(", ");
        let sql = format!(
            "DELETE FROM session_read_cache WHERE session_id = ?1 AND path NOT IN ({placeholders})"
        );
        let mut params_vec: Vec<Box<dyn rusqlite::ToSql>> = vec![Box::new(session_id.to_string())];
        for p in keep_paths {
            params_vec.push(Box::new(p.clone()));
        }
        let refs: Vec<&dyn rusqlite::ToSql> = params_vec.iter().map(|b| b.as_ref()).collect();
        conn.execute(&sql, refs.as_slice())?;
        Ok(())
    }

    pub fn clear_session_read_cache(&self, session_id: &str) -> Result<(), StateError> {
        let conn = self.pool.get()?;
        conn.execute(
            "DELETE FROM session_read_cache WHERE session_id = ?1",
            params![session_id],
        )?;
        Ok(())
    }

    pub fn get_read_cache_anchor(
        &self,
        session_id: &str,
    ) -> Result<Option<ReadCacheAnchor>, StateError> {
        let conn = self.pool.get()?;
        let mut stmt = conn.prepare(
            "SELECT read_cache_compaction_count, read_cache_anchor_turn, read_cache_anchor_sequence
             FROM sessions WHERE id = ?1",
        )?;
        let mut rows = stmt.query(params![session_id])?;
        if let Some(row) = rows.next()? {
            let count: Option<i32> = row.get(0)?;
            let turn: Option<i32> = row.get(1)?;
            let seq: Option<i32> = row.get(2)?;
            return Ok(match (count, turn, seq) {
                (Some(c), Some(t), Some(s)) => Some(ReadCacheAnchor {
                    compaction_count: c,
                    anchor_turn: t,
                    anchor_sequence: s,
                }),
                _ => None,
            });
        }
        Ok(None)
    }

    pub fn set_read_cache_anchor(
        &self,
        session_id: &str,
        anchor: &ReadCacheAnchor,
    ) -> Result<(), StateError> {
        let conn = self.pool.get()?;
        let n = conn.execute(
            "UPDATE sessions SET
               read_cache_compaction_count = ?1,
               read_cache_anchor_turn = ?2,
               read_cache_anchor_sequence = ?3
             WHERE id = ?4",
            params![
                anchor.compaction_count,
                anchor.anchor_turn,
                anchor.anchor_sequence,
                session_id
            ],
        )?;
        if n == 0 {
            return Err(StateError::SessionNotFound(session_id.into()));
        }
        Ok(())
    }

    pub fn clear_read_cache_anchor(&self, session_id: &str) -> Result<(), StateError> {
        let conn = self.pool.get()?;
        conn.execute(
            "UPDATE sessions SET
               read_cache_compaction_count = NULL,
               read_cache_anchor_turn = NULL,
               read_cache_anchor_sequence = NULL
             WHERE id = ?1",
            params![session_id],
        )?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn session_read_cache_roundtrip() {
        let tmp = TempDir::new().unwrap();
        let db = Database::open(tmp.path().join("t.db")).unwrap();
        let sid = db.create_session("/proj", "m").unwrap();
        db.upsert_session_read_cache_entry(&sid, "a.md", r#"{"raw":"x"}"#)
            .unwrap();
        let rows = db.list_session_read_cache(&sid).unwrap();
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].0, "a.md");
        db.delete_session_read_cache_paths_not_in(&sid, &[])
            .unwrap();
        assert!(db.list_session_read_cache(&sid).unwrap().is_empty());
    }
}
