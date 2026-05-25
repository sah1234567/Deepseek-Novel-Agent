use crate::StateError;
use chrono::Utc;
use rusqlite::params;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct SessionTodo {
    pub id: String,
    pub content: String,
    pub status: String,
}

pub fn upsert_todos(
    conn: &rusqlite::Connection,
    session_id: &str,
    todos: &[SessionTodo],
    merge: bool,
) -> Result<(), StateError> {
    if !merge {
        conn.execute(
            "DELETE FROM session_todos WHERE session_id = ?1",
            params![session_id],
        )?;
    }
    let now = Utc::now().to_rfc3339();
    for todo in todos {
        conn.execute(
            "INSERT INTO session_todos (session_id, todo_id, content, status, updated_at)
             VALUES (?1, ?2, ?3, ?4, ?5)
             ON CONFLICT(session_id, todo_id) DO UPDATE SET
               content = excluded.content,
               status = excluded.status,
               updated_at = excluded.updated_at",
            params![session_id, todo.id, todo.content, todo.status, now],
        )?;
    }
    Ok(())
}

pub fn list_todos(
    conn: &rusqlite::Connection,
    session_id: &str,
) -> Result<Vec<SessionTodo>, StateError> {
    let mut stmt = conn.prepare(
        "SELECT todo_id, content, status FROM session_todos
         WHERE session_id = ?1 ORDER BY updated_at ASC",
    )?;
    let rows = stmt.query_map(params![session_id], |row| {
        Ok(SessionTodo {
            id: row.get(0)?,
            content: row.get(1)?,
            status: row.get(2)?,
        })
    })?;
    Ok(rows.filter_map(|r| r.ok()).collect())
}
