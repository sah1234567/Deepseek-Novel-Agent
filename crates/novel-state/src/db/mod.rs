use crate::StateError;
use r2d2_sqlite::SqliteConnectionManager;
use std::path::Path;
use std::time::Duration;

pub(crate) type Pool = r2d2::Pool<SqliteConnectionManager>;

mod fork_runs;
mod messages;
mod metadata;
mod migrate;
mod sessions;

#[cfg(test)]
mod tests;

pub struct Database {
    pub(crate) pool: Pool,
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
        let manager = SqliteConnectionManager::file(path).with_init(|c| {
            c.busy_timeout(Duration::from_secs(5))?;
            c.execute_batch(
                "PRAGMA journal_mode=WAL;
                 PRAGMA synchronous=NORMAL;
                 PRAGMA busy_timeout=5000;",
            )?;
            Ok(())
        });
        let pool = Pool::builder().max_size(8).build(manager).map_err(|e| {
            tracing::error!(path = %path.display(), %e, "failed to create database pool");
            e
        })?;
        let db = Self { pool };
        db.migrate().map_err(|e| {
            tracing::error!(path = %path.display(), %e, "database migration failed");
            e
        })?;
        tracing::debug!(path = %path.display(), "database opened");
        Ok(db)
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
