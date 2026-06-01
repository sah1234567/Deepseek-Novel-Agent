use crate::{message::StoredMessage, session::Session, StateError};
use chrono::Utc;
use r2d2_sqlite::SqliteConnectionManager;
use rusqlite::params;
use std::path::Path;
use uuid::Uuid;

type Pool = r2d2::Pool<SqliteConnectionManager>;

const MIGRATIONS: &[&str] = &[
    "CREATE TABLE IF NOT EXISTS schema_version (version INTEGER NOT NULL);",
    "INSERT OR IGNORE INTO schema_version (version) VALUES (1);",
    r#"
    CREATE TABLE IF NOT EXISTS sessions (
        id TEXT PRIMARY KEY,
        project_root TEXT NOT NULL,
        title TEXT,
        status TEXT NOT NULL DEFAULT 'active',
        model TEXT NOT NULL,
        provider TEXT NOT NULL DEFAULT 'deepseek',
        created_at TEXT NOT NULL,
        last_active_at TEXT NOT NULL,
        cache_hit_tokens INTEGER DEFAULT 0,
        cache_miss_tokens INTEGER DEFAULT 0,
        completion_tokens INTEGER DEFAULT 0,
        context_tokens INTEGER DEFAULT 0,
        total_turns INTEGER DEFAULT 0,
        metadata_json TEXT
    );
    "#,
    r#"
    CREATE TABLE IF NOT EXISTS messages (
        id TEXT PRIMARY KEY,
        session_id TEXT NOT NULL REFERENCES sessions(id) ON DELETE CASCADE,
        turn_number INTEGER NOT NULL,
        sequence INTEGER NOT NULL,
        role TEXT NOT NULL,
        content_json TEXT NOT NULL,
        cache_hit_tokens INTEGER DEFAULT 0,
        cache_miss_tokens INTEGER DEFAULT 0,
        completion_tokens INTEGER DEFAULT 0,
        estimated_tokens INTEGER,
        created_at TEXT NOT NULL,
        UNIQUE(session_id, turn_number, sequence)
    );
    "#,
    "CREATE INDEX IF NOT EXISTS idx_messages_session ON messages(session_id, turn_number, sequence);",
    r#"
    CREATE TABLE IF NOT EXISTS session_todos (
        session_id TEXT NOT NULL REFERENCES sessions(id) ON DELETE CASCADE,
        todo_id TEXT NOT NULL,
        content TEXT NOT NULL,
        status TEXT NOT NULL,
        updated_at TEXT NOT NULL,
        PRIMARY KEY (session_id, todo_id)
    );
    "#,
    // Sub-agent transcript (isolated from parent `messages`; never fed back into main LLM context).
    r#"
    CREATE TABLE IF NOT EXISTS fork_runs (
        id TEXT PRIMARY KEY,
        session_id TEXT NOT NULL REFERENCES sessions(id) ON DELETE CASCADE,
        parent_turn_number INTEGER NOT NULL,
        agent_type TEXT NOT NULL,
        task TEXT NOT NULL,
        source TEXT NOT NULL,
        status TEXT NOT NULL DEFAULT 'running',
        report_message_id TEXT,
        started_at TEXT NOT NULL,
        finished_at TEXT
    );
    "#,
    r#"
    CREATE TABLE IF NOT EXISTS fork_messages (
        id TEXT PRIMARY KEY,
        run_id TEXT NOT NULL REFERENCES fork_runs(id) ON DELETE CASCADE,
        sequence INTEGER NOT NULL,
        role TEXT NOT NULL,
        content_json TEXT NOT NULL,
        created_at TEXT NOT NULL,
        UNIQUE(run_id, sequence)
    );
    "#,
    "CREATE INDEX IF NOT EXISTS idx_fork_messages_run ON fork_messages(run_id, sequence);",
    r#"
    CREATE TABLE IF NOT EXISTS message_archive (
        id TEXT PRIMARY KEY,
        session_id TEXT NOT NULL REFERENCES sessions(id) ON DELETE CASCADE,
        compaction_epoch INTEGER NOT NULL,
        turn_number INTEGER NOT NULL,
        sequence INTEGER NOT NULL,
        role TEXT NOT NULL,
        content_json TEXT NOT NULL,
        archived_at TEXT NOT NULL,
        UNIQUE(session_id, compaction_epoch, turn_number, sequence)
    );
    "#,
    "CREATE INDEX IF NOT EXISTS idx_message_archive_session ON message_archive(session_id, compaction_epoch, turn_number, sequence);",
    "UPDATE schema_version SET version = 2 WHERE version < 2;",
];

pub struct Database {
    pool: Pool,
}

impl Clone for Database {
    fn clone(&self) -> Self {
        Self {
            pool: self.pool.clone(),
        }
    }
}

impl Database {
    pub fn open(path: impl AsRef<Path>) -> Result<Self, StateError> {
        let path = path.as_ref();
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).map_err(StateError::from)?;
        }
        let manager = SqliteConnectionManager::file(path);
        let pool = Pool::builder().max_size(8).build(manager)?;
        let db = Self { pool };
        db.migrate()?;
        Ok(db)
    }

    fn migrate(&self) -> Result<(), StateError> {
        let conn = self.pool.get()?;
        conn.execute_batch("PRAGMA journal_mode=WAL;")?;
        for sql in MIGRATIONS {
            conn.execute_batch(sql)?;
        }
        Self::drop_legacy_total_tokens_column(&conn)?;
        Self::ensure_context_tokens_column(&conn)?;
        Self::ensure_api_call_count_column(&conn)?;
        Self::drop_unused_legacy_schema(&conn)?;
        Self::ensure_message_archive_table(&conn)?;
        Ok(())
    }

    fn ensure_message_archive_table(conn: &rusqlite::Connection) -> Result<(), StateError> {
        conn.execute_batch(
            r#"
            CREATE TABLE IF NOT EXISTS message_archive (
                id TEXT PRIMARY KEY,
                session_id TEXT NOT NULL REFERENCES sessions(id) ON DELETE CASCADE,
                compaction_epoch INTEGER NOT NULL,
                turn_number INTEGER NOT NULL,
                sequence INTEGER NOT NULL,
                role TEXT NOT NULL,
                content_json TEXT NOT NULL,
                archived_at TEXT NOT NULL,
                UNIQUE(session_id, compaction_epoch, turn_number, sequence)
            );
            CREATE INDEX IF NOT EXISTS idx_message_archive_session
                ON message_archive(session_id, compaction_epoch, turn_number, sequence);
            UPDATE schema_version SET version = 2 WHERE version < 2;
            "#,
        )?;
        Ok(())
    }

    /// Remove pre-fork_runs schema (checkpoints / sub_agent_runs / daily_token_stats); never written in production.
    fn drop_unused_legacy_schema(conn: &rusqlite::Connection) -> Result<(), StateError> {
        conn.execute_batch(
            "DROP VIEW IF EXISTS daily_token_stats;
             DROP TABLE IF EXISTS sub_agent_runs;
             DROP TABLE IF EXISTS checkpoints;",
        )?;
        Ok(())
    }

    fn ensure_context_tokens_column(conn: &rusqlite::Connection) -> Result<(), StateError> {
        let mut stmt = conn.prepare("PRAGMA table_info(sessions)")?;
        let mut rows = stmt.query([])?;
        while let Some(row) = rows.next()? {
            let name: String = row.get(1)?;
            if name == "context_tokens" {
                return Ok(());
            }
        }
        conn.execute(
            "ALTER TABLE sessions ADD COLUMN context_tokens INTEGER DEFAULT 0",
            [],
        )?;
        Ok(())
    }

    /// Split legacy `total_turns` (was LLM call count) into `api_call_count` + user `total_turns`.
    fn ensure_api_call_count_column(conn: &rusqlite::Connection) -> Result<(), StateError> {
        let mut stmt = conn.prepare("PRAGMA table_info(sessions)")?;
        let mut rows = stmt.query([])?;
        while let Some(row) = rows.next()? {
            let name: String = row.get(1)?;
            if name == "api_call_count" {
                return Ok(());
            }
        }
        conn.execute(
            "ALTER TABLE sessions ADD COLUMN api_call_count INTEGER NOT NULL DEFAULT 0",
            [],
        )?;
        // Historical DBs stored LLM call count in `total_turns`.
        conn.execute("UPDATE sessions SET api_call_count = total_turns", [])?;
        conn.execute(
            "UPDATE sessions SET total_turns = COALESCE((
                SELECT MAX(turn_number) FROM messages
                WHERE messages.session_id = sessions.id AND role = 'user'
            ), 0)",
            [],
        )?;
        Ok(())
    }

    /// Remove deprecated `total_tokens` from databases created before v2 schema.
    fn drop_legacy_total_tokens_column(conn: &rusqlite::Connection) -> Result<(), StateError> {
        let mut stmt = conn.prepare("PRAGMA table_info(sessions)")?;
        let mut rows = stmt.query([])?;
        let mut has_total_tokens = false;
        while let Some(row) = rows.next()? {
            let name: String = row.get(1)?;
            if name == "total_tokens" {
                has_total_tokens = true;
                break;
            }
        }
        if has_total_tokens {
            conn.execute("ALTER TABLE sessions DROP COLUMN total_tokens", [])?;
        }
        Ok(())
    }

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
    pub fn accumulate_session_tokens(
        &self,
        session_id: &str,
        hit: i64,
        miss: i64,
        completion: i64,
        last_model: &str,
    ) -> Result<(), StateError> {
        let conn = self.pool.get()?;
        let n = conn.execute(
            "UPDATE sessions SET
                cache_hit_tokens = cache_hit_tokens + ?1,
                cache_miss_tokens = cache_miss_tokens + ?2,
                completion_tokens = completion_tokens + ?3,
                context_tokens = ?1 + ?2 + ?3,
                api_call_count = api_call_count + 1,
                last_active_at = ?4,
                model = ?5
             WHERE id = ?6",
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
        let conn = self.pool.get()?;
        let mut stmt = conn.prepare(
            "SELECT id, session_id, turn_number, sequence, role, content_json,
                    0, 0, 0, NULL, archived_at
             FROM message_archive
             WHERE session_id = ?1 AND compaction_epoch = ?2
             ORDER BY turn_number, sequence",
        )?;
        let rows = stmt.query(params![session_id, compaction_epoch])?;
        map_messages(rows)
    }

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
        let rows = stmt.query_map(params![run_id], |row| {
            let created: String = row.get(5)?;
            let content_str: String = row.get(4)?;
            Ok(crate::ForkMessage {
                id: row.get(0)?,
                run_id: row.get(1)?,
                sequence: row.get(2)?,
                role: row.get(3)?,
                content_json: serde_json::from_str(&content_str).unwrap_or(serde_json::Value::Null),
                created_at: created.parse().unwrap_or_else(|_| Utc::now()),
            })
        })?;
        Ok(rows.filter_map(|r| r.ok()).collect())
    }

    pub fn upsert_session_todos(
        &self,
        session_id: &str,
        todos: &[crate::SessionTodo],
        merge: bool,
    ) -> Result<(), StateError> {
        let conn = self.pool.get()?;
        crate::todo::upsert_todos(&conn, session_id, todos, merge)
    }

    pub fn list_session_todos(
        &self,
        session_id: &str,
    ) -> Result<Vec<crate::SessionTodo>, StateError> {
        let conn = self.pool.get()?;
        crate::todo::list_todos(&conn, session_id)
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

#[cfg(test)]
mod tests {
    use super::*;
    use rstest::rstest;
    use std::sync::Arc;
    use tempfile::TempDir;

    fn test_db() -> Database {
        let tmp = TempDir::new().unwrap();
        Database::open(tmp.path().join("test.db")).unwrap()
    }

    #[rstest]
    #[test]
    fn migration_creates_tables() {
        let db = test_db();
        let tables = db.list_tables().unwrap();
        assert!(tables.contains(&"sessions".to_string()));
        assert!(tables.contains(&"messages".to_string()));
        assert!(tables.contains(&"fork_runs".to_string()));
        assert!(tables.contains(&"fork_messages".to_string()));
        assert!(tables.contains(&"session_todos".to_string()));
        assert!(tables.contains(&"message_archive".to_string()));
        assert!(!tables.contains(&"checkpoints".to_string()));
        assert!(!tables.contains(&"sub_agent_runs".to_string()));
    }

    #[test]
    fn archive_session_messages_preserves_epoch() {
        let db = test_db();
        let sid = db.create_session("/tmp/proj", "deepseek-chat").unwrap();
        db.insert_message(
            &sid,
            0,
            0,
            "system",
            &serde_json::json!({"role":"system","content":"sys"}),
            None,
        )
        .unwrap();
        db.insert_message(
            &sid,
            1,
            0,
            "user",
            &serde_json::json!({"role":"user","content":"hello"}),
            None,
        )
        .unwrap();
        db.archive_session_messages(&sid, 1).unwrap();
        let epochs = db.get_archived_epochs(&sid).unwrap();
        assert_eq!(epochs, vec![1]);
        let archived = db.get_archived_messages(&sid, 1).unwrap();
        assert_eq!(archived.len(), 2);
        db.replace_session_messages(
            &sid,
            &[(
                0,
                0,
                "system",
                &serde_json::json!({"role":"system","content":"new"}),
            )],
        )
        .unwrap();
        assert_eq!(db.get_session_messages(&sid, None).unwrap().len(), 1);
        assert_eq!(db.get_archived_messages(&sid, 1).unwrap().len(), 2);
    }

    #[test]
    fn increment_compaction_count_metadata() {
        let db = test_db();
        let sid = db.create_session("/tmp/proj", "deepseek-chat").unwrap();
        assert_eq!(db.get_compaction_count(&sid).unwrap(), 0);
        assert_eq!(db.increment_compaction_count(&sid).unwrap(), 1);
        assert_eq!(db.get_compaction_count(&sid).unwrap(), 1);
    }

    #[test]
    fn replace_session_messages_swaps_history() {
        let db = test_db();
        let sid = db.create_session("/tmp/proj", "deepseek-chat").unwrap();
        for i in 0..5 {
            db.insert_message(
                &sid,
                i,
                0,
                "user",
                &serde_json::json!({"role":"user","content":format!("msg {i}")}),
                None,
            )
            .unwrap();
        }
        let sys = serde_json::json!({"role":"system","content":"sys"});
        let user = serde_json::json!({"role":"user","content":"compact"});
        let replacement = vec![(0i32, 0i32, "system", &sys), (1i32, 0i32, "user", &user)];
        db.replace_session_messages(&sid, &replacement).unwrap();
        let msgs = db.get_session_messages(&sid, None).unwrap();
        assert_eq!(msgs.len(), 2);
        assert_eq!(msgs[0].role, "system");
        assert_eq!(msgs[1].role, "user");
    }

    #[test]
    fn invoked_skill_ids_metadata_roundtrip() {
        let db = test_db();
        let sid = db.create_session("/tmp/proj", "deepseek-chat").unwrap();
        db.set_invoked_skill_ids(&sid, &["plot-grid".into(), "log-integrity-checker".into()])
            .unwrap();
        let ids = db.get_invoked_skill_ids(&sid).unwrap();
        assert_eq!(ids, vec!["plot-grid", "log-integrity-checker"]);
    }

    #[test]
    fn read_skill_reference_paths_metadata_roundtrip() {
        let db = test_db();
        let sid = db.create_session("/tmp/proj", "deepseek-chat").unwrap();
        assert!(db.get_read_skill_reference_paths(&sid).unwrap().is_empty());
        db.set_read_skill_reference_paths(
            &sid,
            &[
                "apocalypse/references/zombie.md".into(),
                "romance/references/harem.md".into(),
            ],
        )
        .unwrap();
        let paths = db.get_read_skill_reference_paths(&sid).unwrap();
        assert_eq!(
            paths,
            vec![
                "apocalypse/references/zombie.md",
                "romance/references/harem.md"
            ]
        );
    }

    #[test]
    fn session_crud() {
        let db = test_db();
        let id = db.create_session("/tmp/proj", "deepseek-chat").unwrap();
        let s = db.get_session(&id).unwrap().unwrap();
        assert_eq!(s.status, "active");
        db.update_session_status(&id, "completed").unwrap();
        let s2 = db.get_session(&id).unwrap().unwrap();
        assert_eq!(s2.status, "completed");
    }

    #[test]
    fn get_nonexistent_session() {
        let db = test_db();
        assert!(db.get_session("nonexistent").unwrap().is_none());
    }

    #[test]
    fn token_accumulates_for_billing() {
        let db = test_db();
        let sid = db.create_session("/tmp/proj", "deepseek-chat").unwrap();
        db.accumulate_session_tokens(&sid, 100, 50, 30, "deepseek-v4-pro")
            .unwrap();
        db.accumulate_session_tokens(&sid, 200, 80, 70, "deepseek-v4-pro")
            .unwrap();
        let s = db.get_session(&sid).unwrap().unwrap();
        assert_eq!(s.cache_hit_tokens, 300);
        assert_eq!(s.cache_miss_tokens, 130);
        assert_eq!(s.completion_tokens, 100);
        assert_eq!(s.api_call_count, 2);
        assert_eq!(s.total_turns, 0);
        assert_eq!(s.model, "deepseek-v4-pro");
    }

    #[test]
    fn sync_user_turn_count_does_not_touch_last_active_at() {
        let db = test_db();
        let sid = db.create_session("/tmp/proj", "deepseek-chat").unwrap();
        let before = db.get_session(&sid).unwrap().unwrap().last_active_at;
        std::thread::sleep(std::time::Duration::from_millis(10));
        db.sync_user_turn_count(&sid, 3).unwrap();
        let after = db.get_session(&sid).unwrap().unwrap().last_active_at;
        assert_eq!(after, before);
    }

    #[test]
    fn accumulate_session_tokens_updates_last_active_at() {
        let db = test_db();
        let sid = db.create_session("/tmp/proj", "deepseek-chat").unwrap();
        let before = db.get_session(&sid).unwrap().unwrap().last_active_at;
        std::thread::sleep(std::time::Duration::from_millis(10));
        db.accumulate_session_tokens(&sid, 1, 0, 0, "deepseek-v4-pro")
            .unwrap();
        let after = db.get_session(&sid).unwrap().unwrap().last_active_at;
        assert!(after > before);
    }

    #[test]
    fn user_turn_count_independent_of_api_calls() {
        let db = test_db();
        let sid = db.create_session("/tmp/proj", "deepseek-chat").unwrap();
        db.sync_user_turn_count(&sid, 1).unwrap();
        db.accumulate_session_tokens(&sid, 10, 5, 2, "deepseek-v4-pro")
            .unwrap();
        db.accumulate_session_tokens(&sid, 10, 5, 2, "deepseek-v4-pro")
            .unwrap();
        db.accumulate_session_tokens(&sid, 10, 5, 2, "deepseek-v4-pro")
            .unwrap();
        let s = db.get_session(&sid).unwrap().unwrap();
        assert_eq!(s.total_turns, 1);
        assert_eq!(s.api_call_count, 3);
    }

    #[test]
    fn message_insert_and_query() {
        let db = test_db();
        let sid = db.create_session("/tmp/proj", "deepseek-chat").unwrap();
        db.insert_message(
            &sid,
            1,
            0,
            "user",
            &serde_json::json!({"content":"hello"}),
            None,
        )
        .unwrap();
        let msgs = db.get_session_messages(&sid, None).unwrap();
        assert_eq!(msgs.len(), 1);
        assert_eq!(msgs[0].role, "user");
    }

    #[test]
    fn message_query_turn_range() {
        let db = test_db();
        let sid = db.create_session("/tmp/proj", "deepseek-chat").unwrap();
        for t in 1..=5 {
            db.insert_message(
                &sid,
                t,
                0,
                "user",
                &serde_json::json!({"content":format!("t{t}")}),
                None,
            )
            .unwrap();
        }
        let msgs = db.get_session_messages(&sid, Some((2, 4))).unwrap();
        assert_eq!(msgs.len(), 3);
    }

    #[test]
    fn list_sessions_filters_by_project_and_orders_by_activity() {
        let db = test_db();
        let a = db.create_session("/proj/a", "deepseek-chat").unwrap();
        let b = db.create_session("/proj/b", "deepseek-chat").unwrap();
        let c = db.create_session("/proj/a", "deepseek-chat").unwrap();
        db.accumulate_session_tokens(&a, 1, 0, 0, "deepseek-v4-pro")
            .unwrap();
        db.accumulate_session_tokens(&b, 1, 0, 0, "deepseek-v4-pro")
            .unwrap();
        db.accumulate_session_tokens(&c, 1, 0, 0, "deepseek-v4-pro")
            .unwrap();
        let list = db.list_sessions("/proj/a", 10).unwrap();
        assert_eq!(list.len(), 2);
        // Most recent first
        assert_eq!(list[0].id, c);
    }

    #[test]
    fn database_recreated_if_deleted() {
        let tmp = TempDir::new().unwrap();
        let p = tmp.path().join("test.db");
        {
            let db = Database::open(&p).unwrap();
            let id = db.create_session("/proj", "deepseek-chat").unwrap();
            db.accumulate_session_tokens(&id, 10, 5, 2, "deepseek-v4-pro")
                .unwrap();
        }
        std::fs::remove_file(&p).unwrap();
        let db = Database::open(&p).unwrap();
        assert_eq!(db.session_count().unwrap(), 0);
    }

    #[test]
    fn fork_run_messages_persist_in_order() {
        let db = test_db();
        let sid = db.create_session("/tmp/proj", "deepseek-chat").unwrap();
        let run_id = db
            .create_fork_run(&sid, 1, "KnowledgeAuditor", "audit ch1", "tool")
            .unwrap();
        db.insert_fork_message(
            &run_id,
            "assistant",
            &serde_json::json!({"content": "checking"}),
        )
        .unwrap();
        db.insert_fork_message(
            &run_id,
            "tool",
            &serde_json::json!({"content": "ok", "tool_call_id": "tc1"}),
        )
        .unwrap();
        let msgs = db.get_fork_messages(&run_id).unwrap();
        assert_eq!(msgs.len(), 2);
        assert_eq!(msgs[0].sequence, 0);
        assert_eq!(msgs[1].sequence, 1);
        db.finish_fork_run(&run_id, "complete", Some("report-id"))
            .unwrap();
        let msgs_after = db.get_fork_messages(&run_id).unwrap();
        assert_eq!(msgs_after.len(), 2);
    }

    #[test]
    fn concurrent_writes() {
        let db = Arc::new(test_db());
        let sid = db.create_session("/proj", "deepseek-chat").unwrap();
        let mut handles = vec![];
        for _ in 0..10 {
            let db = Arc::clone(&db);
            let sid = sid.clone();
            handles.push(std::thread::spawn(move || {
                db.accumulate_session_tokens(&sid, 1, 0, 0, "deepseek-v4-pro")
                    .unwrap();
            }));
        }
        for h in handles {
            h.join().unwrap();
        }
        let s = db.get_session(&sid).unwrap().unwrap();
        // Tokens replace (race ??? last write wins), API calls still increment
        assert!(s.cache_hit_tokens >= 1);
        assert_eq!(s.api_call_count, 10);
        assert_eq!(s.total_turns, 0);
    }
}
