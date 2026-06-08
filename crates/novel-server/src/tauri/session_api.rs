//! Session and work-directory helpers (no Tauri `AppHandle`).

use crate::tauri::dto::{stored_messages_to_turn_bundles, validate_turn_range, UiTurnBundle};

use novel_state::{Database, StateError, StoredMessage};
use novel_tools::PermissionMode;
use std::path::Path;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum TurnMessageSource {
    Active,
    Archive(i32),
}

pub(crate) fn load_turn_range_messages(
    db: &Database,
    session_id: &str,
    from_turn: i32,
    to_turn: i32,
    source: TurnMessageSource,
) -> Result<Vec<StoredMessage>, StateError> {
    let range = Some((from_turn, to_turn));
    match source {
        TurnMessageSource::Active => db.get_session_messages(session_id, range),
        TurnMessageSource::Archive(epoch) => {
            db.get_archived_messages_turn_range(session_id, epoch, range)
        }
    }
}

pub(crate) fn bundles_for_turn_range(
    db: &Database,
    session_id: &str,
    from_turn: i32,
    to_turn: i32,
    source: TurnMessageSource,
) -> Result<Vec<UiTurnBundle>, String> {
    validate_turn_range(from_turn, to_turn)?;
    let stored = load_turn_range_messages(db, session_id, from_turn, to_turn, source)
        .map_err(|e| e.to_string())?;
    let bundles = stored_messages_to_turn_bundles(&stored);
    trace_turn_bundles_loaded(session_id, source, from_turn, to_turn, bundles.len());
    Ok(bundles)
}

pub(crate) fn trace_turn_bundles_loaded(
    session_id: &str,
    source: TurnMessageSource,
    from_turn: i32,
    to_turn: i32,
    bundle_count: usize,
) {
    match source {
        TurnMessageSource::Active => tracing::debug!(
            session_id = %session_id,
            from_turn,
            to_turn,
            bundle_count,
            "get_session_message_turns"
        ),
        TurnMessageSource::Archive(epoch) => tracing::debug!(
            session_id = %session_id,
            epoch,
            from_turn,
            to_turn,
            bundle_count,
            "get_session_archive_turns"
        ),
    }
}

/// Strict permission mode parse for IPC (`set_permission_mode`): unknown values are rejected.
///
/// Settings file load uses `PermissionMode::from_settings_str` (unknown → `Normal`).
pub(crate) fn parse_permission_mode(mode: &str) -> Result<PermissionMode, String> {
    PermissionMode::try_from_ipc(mode)
}

/// Sorted work directory names under `works_root` (non-hidden directories only).
pub(crate) fn list_work_dirs(works_root: &Path) -> Vec<String> {
    if !works_root.is_dir() {
        return Vec::new();
    }
    let Ok(entries) = std::fs::read_dir(works_root) else {
        return Vec::new();
    };
    let mut names = Vec::new();
    for entry in entries.flatten() {
        let path = entry.path();
        if !path.is_dir() {
            continue;
        }
        let Some(name) = path
            .file_name()
            .and_then(|n| n.to_str())
            .filter(|n| !n.is_empty() && !n.starts_with('.'))
        else {
            continue;
        };
        names.push(name.to_string());
    }
    names.sort();
    names
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::TempDir;

    #[test]
    fn empty_when_root_missing() {
        assert!(list_work_dirs(Path::new("/nonexistent/works/root")).is_empty());
    }

    #[test]
    fn skips_hidden_files_and_non_dirs() {
        let tmp = TempDir::new().unwrap();
        let root = tmp.path();
        fs::create_dir(root.join("beta")).unwrap();
        fs::create_dir(root.join("alpha")).unwrap();
        fs::create_dir(root.join(".hidden")).unwrap();
        fs::write(root.join("not-a-dir.txt"), "x").unwrap();
        assert_eq!(
            list_work_dirs(root),
            vec!["alpha".to_string(), "beta".to_string()]
        );
    }

    #[test]
    fn parse_permission_mode_accepts_known_values() {
        assert!(matches!(
            parse_permission_mode("auto").unwrap(),
            PermissionMode::Auto
        ));
        assert!(matches!(
            parse_permission_mode("unattended").unwrap(),
            PermissionMode::Unattended
        ));
        assert!(parse_permission_mode("bogus").is_err());
    }

    #[test]
    fn empty_when_root_is_file() {
        let tmp = TempDir::new().unwrap();
        let f = tmp.path().join("file");
        fs::write(&f, "x").unwrap();
        assert!(list_work_dirs(&f).is_empty());
    }

    #[test]
    fn load_turn_range_messages_reads_active_turns() {
        let tmp = TempDir::new().unwrap();
        let db = novel_state::Database::open(tmp.path().join("t.db")).unwrap();
        let sid = db
            .create_session(tmp.path().to_str().unwrap(), "m")
            .unwrap();
        db.insert_message(
            &sid,
            1,
            0,
            "user",
            &serde_json::json!({"content": "hi"}),
            None,
        )
        .unwrap();
        let msgs = load_turn_range_messages(&db, &sid, 1, 1, TurnMessageSource::Active).unwrap();
        assert_eq!(msgs.len(), 1);
        assert_eq!(msgs[0].turn_number, 1);
    }

    #[test]
    fn bundles_for_turn_range_rejects_inverted_range() {
        let tmp = TempDir::new().unwrap();
        let db = novel_state::Database::open(tmp.path().join("t.db")).unwrap();
        let sid = db
            .create_session(tmp.path().to_str().unwrap(), "m")
            .unwrap();
        let err = bundles_for_turn_range(&db, &sid, 3, 1, TurnMessageSource::Active).unwrap_err();
        assert!(err.contains("fromTurn"));
    }

    #[test]
    fn bundles_for_turn_range_groups_messages() {
        let tmp = TempDir::new().unwrap();
        let db = novel_state::Database::open(tmp.path().join("t.db")).unwrap();
        let sid = db
            .create_session(tmp.path().to_str().unwrap(), "m")
            .unwrap();
        db.insert_message(
            &sid,
            1,
            0,
            "user",
            &serde_json::json!({"content": "hi"}),
            None,
        )
        .unwrap();
        db.insert_message(
            &sid,
            1,
            1,
            "assistant",
            &serde_json::json!({"content": "hello"}),
            None,
        )
        .unwrap();
        let bundles = bundles_for_turn_range(&db, &sid, 1, 1, TurnMessageSource::Active).unwrap();
        assert_eq!(bundles.len(), 1);
        assert_eq!(bundles[0].turn_number, 1);
        assert_eq!(bundles[0].messages.len(), 2);
    }
}
