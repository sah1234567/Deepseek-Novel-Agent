use super::Database;
use crate::StateError;

pub(super) const MIGRATIONS: &[&str] = &[
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

impl Database {
    pub(crate) fn migrate(&self) -> Result<(), StateError> {
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
}
