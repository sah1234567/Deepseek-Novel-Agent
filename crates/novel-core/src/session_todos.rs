//! Push session todo snapshots to the UI without blocking on `get_app_status`.

use crate::Event;
use novel_state::Database;
use tokio::sync::mpsc;

pub(crate) fn emit_session_todos_updated(
    session_id: &str,
    db: &Database,
    event_tx: Option<&mpsc::UnboundedSender<Event>>,
) {
    let Some(tx) = event_tx else {
        return;
    };
    match db.list_session_todos(session_id) {
        Ok(todos) => {
            let _ = tx.send(Event::SessionTodosUpdated { todos });
        }
        Err(e) => {
            tracing::warn!(
                session_id = %session_id,
                error = %e,
                "list_session_todos failed; skipping SessionTodosUpdated emit"
            );
        }
    }
}

pub(crate) fn maybe_emit_session_todos_after_tool(
    tool_name: &str,
    session_id: &str,
    db: &Database,
    event_tx: Option<&mpsc::UnboundedSender<Event>>,
) {
    if tool_name == "TodoWrite" {
        emit_session_todos_updated(session_id, db, event_tx);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Event;
    use tempfile::TempDir;
    use tokio::sync::mpsc;

    #[test]
    fn emit_session_todos_updated_sends_list() {
        let tmp = TempDir::new().unwrap();
        let db = Database::open(tmp.path().join("todos.db")).unwrap();
        let sid = db.create_session("/tmp/proj", "deepseek-chat").unwrap();
        db.upsert_session_todos(
            &sid,
            &[novel_state::SessionTodo {
                id: "t1".into(),
                content: "plan chapter".into(),
                status: "in_progress".into(),
            }],
            true, // replace: seed test data
        )
        .unwrap();

        let (tx, mut rx) = mpsc::unbounded_channel();
        emit_session_todos_updated(&sid, &db, Some(&tx));
        let event = rx.try_recv().expect("SessionTodosUpdated");
        match event {
            Event::SessionTodosUpdated { todos } => {
                assert_eq!(todos.len(), 1);
                assert_eq!(todos[0].content, "plan chapter");
            }
            other => panic!("unexpected event: {other:?}"),
        }
    }
}
