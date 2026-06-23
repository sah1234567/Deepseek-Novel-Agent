use crate::StateError;
use chrono::Utc;
use rusqlite::params;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct SessionTodo {
    pub id: String,
    pub content: String,
    pub status: String,
}

impl SessionTodo {
    /// True when this todo still needs work (not terminal).
    pub fn is_unfinished(&self) -> bool {
        self.status == "pending" || self.status == "in_progress"
    }
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum TodoValidationError {
    #[error("at most one todo may be in_progress (found {count})")]
    MultipleInProgress { count: usize },
}

/// For `replace=false`: split incoming into rows that exist vs ids to skip.
pub fn partition_status_updates(
    existing: &[SessionTodo],
    incoming: &[SessionTodo],
) -> (Vec<SessionTodo>, Vec<String>) {
    let existing_ids: std::collections::HashSet<&str> =
        existing.iter().map(|t| t.id.as_str()).collect();
    let mut apply = Vec::new();
    let mut skipped = Vec::new();
    for t in incoming {
        if existing_ids.contains(t.id.as_str()) {
            apply.push(t.clone());
        } else {
            skipped.push(t.id.clone());
        }
    }
    (apply, skipped)
}

/// Project the session todo list after a TodoWrite upsert (before persisting).
pub fn project_todos_after_upsert(
    existing: &[SessionTodo],
    incoming: &[SessionTodo],
    replace: bool,
) -> Vec<SessionTodo> {
    if replace {
        return incoming.to_vec();
    }
    let mut by_id: HashMap<String, SessionTodo> =
        existing.iter().map(|t| (t.id.clone(), t.clone())).collect();
    for t in incoming {
        if let Some(slot) = by_id.get_mut(&t.id) {
            *slot = t.clone();
        }
    }
    existing
        .iter()
        .filter_map(|t| by_id.get(&t.id).cloned())
        .collect()
}

/// Rules enforced for TodoWrite.
pub fn validate_todo_upsert(
    existing: &[SessionTodo],
    incoming: &[SessionTodo],
    replace: bool,
) -> Result<(), TodoValidationError> {
    let projected = project_todos_after_upsert(existing, incoming, replace);
    let in_progress = projected
        .iter()
        .filter(|t| t.status == "in_progress")
        .count();
    if in_progress > 1 {
        return Err(TodoValidationError::MultipleInProgress { count: in_progress });
    }
    Ok(())
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
    replace: bool,
) -> Result<(), StateError> {
    if replace {
        conn.execute(
            "DELETE FROM session_todos WHERE session_id = ?1",
            params![session_id],
        )?;
    }
    let now = Utc::now().to_rfc3339();
    for (index, todo) in todos.iter().enumerate() {
        let position = if replace {
            index as i64
        } else {
            let Some(position) = existing_todo_position(conn, session_id, &todo.id)? else {
                // Status-update path: unknown ids are skipped (TodoWrite partitions before call).
                continue;
            };
            position
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn project_todos_update_only_mutates_existing_ids() {
        let existing = vec![
            SessionTodo {
                id: "a".into(),
                content: "old".into(),
                status: "pending".into(),
            },
            SessionTodo {
                id: "b".into(),
                content: "b".into(),
                status: "completed".into(),
            },
        ];
        let incoming = vec![SessionTodo {
            id: "a".into(),
            content: "new".into(),
            status: "in_progress".into(),
        }];
        let projected = project_todos_after_upsert(&existing, &incoming, false);
        assert_eq!(projected.len(), 2);
        assert_eq!(projected[0].id, "a");
        assert_eq!(projected[0].status, "in_progress");
    }

    #[test]
    fn project_todos_replace_replaces_entire_list() {
        let existing = vec![SessionTodo {
            id: "a".into(),
            content: "a".into(),
            status: "pending".into(),
        }];
        let incoming = vec![SessionTodo {
            id: "x".into(),
            content: "x".into(),
            status: "pending".into(),
        }];
        let projected = project_todos_after_upsert(&existing, &incoming, true);
        assert_eq!(projected.len(), 1);
        assert_eq!(projected[0].id, "x");
    }

    #[test]
    fn partition_status_updates_splits_known_and_unknown() {
        let existing = vec![SessionTodo {
            id: "1".into(),
            content: "work".into(),
            status: "in_progress".into(),
        }];
        let incoming = vec![
            SessionTodo {
                id: "1".into(),
                content: "work".into(),
                status: "completed".into(),
            },
            SessionTodo {
                id: "2".into(),
                content: "new".into(),
                status: "pending".into(),
            },
        ];
        let (apply, skipped) = partition_status_updates(&existing, &incoming);
        assert_eq!(apply.len(), 1);
        assert_eq!(apply[0].status, "completed");
        assert_eq!(skipped, vec!["2".to_string()]);
    }

    #[test]
    fn validate_allows_status_update_on_existing() {
        let existing = vec![SessionTodo {
            id: "1".into(),
            content: "work".into(),
            status: "in_progress".into(),
        }];
        let incoming = vec![SessionTodo {
            id: "1".into(),
            content: "work".into(),
            status: "completed".into(),
        }];
        validate_todo_upsert(&existing, &incoming, false).unwrap();
    }

    #[test]
    fn validate_rejects_second_in_progress_via_status_update() {
        let existing = vec![
            SessionTodo {
                id: "1".into(),
                content: "a".into(),
                status: "in_progress".into(),
            },
            SessionTodo {
                id: "2".into(),
                content: "b".into(),
                status: "pending".into(),
            },
        ];
        let incoming = vec![SessionTodo {
            id: "2".into(),
            content: "b".into(),
            status: "in_progress".into(),
        }];
        let err = validate_todo_upsert(&existing, &incoming, false).unwrap_err();
        assert!(matches!(
            err,
            TodoValidationError::MultipleInProgress { count: 2 }
        ));
    }

    #[test]
    fn validate_rejects_multiple_in_progress_in_new_batch() {
        let incoming = vec![
            SessionTodo {
                id: "1".into(),
                content: "a".into(),
                status: "in_progress".into(),
            },
            SessionTodo {
                id: "2".into(),
                content: "b".into(),
                status: "in_progress".into(),
            },
        ];
        let err = validate_todo_upsert(&[], &incoming, true).unwrap_err();
        assert!(matches!(
            err,
            TodoValidationError::MultipleInProgress { count: 2 }
        ));
    }
}
