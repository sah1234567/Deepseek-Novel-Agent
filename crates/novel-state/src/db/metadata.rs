use super::Database;
use crate::StateError;
use rusqlite::params;

impl Database {
    pub fn get_compaction_count(&self, session_id: &str) -> Result<i32, StateError> {
        Ok(self
            .get_session_metadata(session_id)?
            .and_then(|v| v.get("compaction_count").and_then(|n| n.as_i64()))
            .unwrap_or(0) as i32)
    }

    /// Increment compaction counter and return the new epoch used for archiving.
    pub fn increment_compaction_count(&self, session_id: &str) -> Result<i32, StateError> {
        let next = self.get_compaction_count(session_id)? + 1;
        let mut meta = self
            .get_session_metadata(session_id)?
            .unwrap_or_else(|| serde_json::json!({}));
        if let Some(obj) = meta.as_object_mut() {
            obj.insert("compaction_count".into(), serde_json::json!(next));
        }
        self.set_session_metadata(session_id, &meta)?;
        Ok(next)
    }

    pub fn require_frozen_system_metadata(&self, session_id: &str) -> Result<(), StateError> {
        let meta = self
            .get_session_metadata(session_id)?
            .ok_or_else(|| StateError::Validation(format!(
                "session {session_id} missing metadata; run scripts/reset-work-databases and create a new session"
            )))?;
        if !meta
            .get("system_static_frozen")
            .and_then(|v| v.as_bool())
            .unwrap_or(false)
        {
            return Err(StateError::Validation(format!(
                "session {session_id} uses legacy format; run scripts/reset-work-databases and create a new session"
            )));
        }
        Ok(())
    }

    pub fn get_session_metadata(
        &self,
        session_id: &str,
    ) -> Result<Option<serde_json::Value>, StateError> {
        let conn = self.pool.get()?;
        let mut stmt = conn.prepare("SELECT metadata_json FROM sessions WHERE id = ?1")?;
        let mut rows = stmt.query(params![session_id])?;
        if let Some(row) = rows.next()? {
            let raw: Option<String> = row.get(0)?;
            if let Some(s) = raw {
                let v: serde_json::Value = serde_json::from_str(&s)?;
                return Ok(Some(v));
            }
        }
        Ok(None)
    }

    pub fn set_session_metadata(
        &self,
        session_id: &str,
        metadata: &serde_json::Value,
    ) -> Result<(), StateError> {
        let conn = self.pool.get()?;
        let json = serde_json::to_string(metadata)?;
        let n = conn.execute(
            "UPDATE sessions SET metadata_json = ?1 WHERE id = ?2",
            params![json, session_id],
        )?;
        if n == 0 {
            return Err(StateError::SessionNotFound(session_id.into()));
        }
        Ok(())
    }

    pub fn get_invoked_skill_ids(&self, session_id: &str) -> Result<Vec<String>, StateError> {
        Ok(self
            .get_session_metadata(session_id)?
            .and_then(|v| {
                v.get("invoked_skill_ids")
                    .and_then(|a| a.as_array())
                    .map(|arr| {
                        arr.iter()
                            .filter_map(|x| x.as_str().map(String::from))
                            .collect()
                    })
            })
            .unwrap_or_default())
    }

    pub fn set_invoked_skill_ids(
        &self,
        session_id: &str,
        ids: &[String],
    ) -> Result<(), StateError> {
        let mut meta = self
            .get_session_metadata(session_id)?
            .unwrap_or_else(|| serde_json::json!({}));
        if let Some(obj) = meta.as_object_mut() {
            obj.insert(
                "invoked_skill_ids".into(),
                serde_json::Value::Array(
                    ids.iter()
                        .map(|s| serde_json::Value::String(s.clone()))
                        .collect(),
                ),
            );
        }
        self.set_session_metadata(session_id, &meta)
    }

    pub fn get_read_skill_reference_paths(
        &self,
        session_id: &str,
    ) -> Result<Vec<String>, StateError> {
        Ok(self
            .get_session_metadata(session_id)?
            .and_then(|v| {
                v.get("read_skill_reference_paths")
                    .and_then(|a| a.as_array())
                    .map(|arr| {
                        arr.iter()
                            .filter_map(|x| x.as_str().map(String::from))
                            .collect()
                    })
            })
            .unwrap_or_default())
    }

    pub fn set_read_skill_reference_paths(
        &self,
        session_id: &str,
        paths: &[String],
    ) -> Result<(), StateError> {
        let mut meta = self
            .get_session_metadata(session_id)?
            .unwrap_or_else(|| serde_json::json!({}));
        if let Some(obj) = meta.as_object_mut() {
            obj.insert(
                "read_skill_reference_paths".into(),
                serde_json::Value::Array(
                    paths
                        .iter()
                        .map(|s| serde_json::Value::String(s.clone()))
                        .collect(),
                ),
            );
        }
        self.set_session_metadata(session_id, &meta)
    }
}
