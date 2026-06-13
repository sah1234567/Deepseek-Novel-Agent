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

fn next_todo_position(conn: &rusqlite::Connection, session_id: &str) -> Result<i64, StateError> {
    conn.query_row(
        "SELECT COALESCE(MAX(position), -1) + 1 FROM session_todos WHERE session_id = ?1",
        params![session_id],
        |row| row.get(0),
    )
    .map_err(Into::into)
}

fn existing_todo_position(
    conn: &rusqlite::Connection,
    session_id: &str,
    todo_id: &str,
) -> Result<Option<i64>, StateError> {
    match conn.query_row(
        "SELECT position FROM session_todos WHERE session_id = ?1 AND todo_id = ?2",
        params![session_id, todo_id],
        |row| row.get(0),
    ) {
        Ok(position) => Ok(Some(position)),
        Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
        Err(e) => Err(e.into()),
    }
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
    let mut next_new_position = next_todo_position(conn, session_id)?;
    for (index, todo) in todos.iter().enumerate() {
        let position = if merge {
            match existing_todo_position(conn, session_id, &todo.id)? {
                Some(position) => position,
                None => {
                    let position = next_new_position;
                    next_new_position += 1;
                    position
                }
            }
        } else {
            index as i64
        };
        conn.execute(
            "INSERT INTO session_todos (session_id, todo_id, content, status, updated_at, position)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6)
             ON CONFLICT(session_id, todo_id) DO UPDATE SET
               content = excluded.content,
               status = excluded.status,
               updated_at = excluded.updated_at",
            params![
                session_id,
                todo.id,
                todo.content,
                todo.status,
                now,
                position
            ],
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
         WHERE session_id = ?1 ORDER BY position ASC, todo_id ASC",
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
