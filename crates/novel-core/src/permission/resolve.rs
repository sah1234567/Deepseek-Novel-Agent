//! Resume-time session permission mode resolution.

use novel_state::{Database, StateError, StoredMessage};
use novel_tools::PermissionMode;

use super::mode_prompt::permission_notice_direction;

const CONTEXT_REFRESH_PREFIX: &str = "[上下文刷新]";

/// Resolve live permission mode when opening an existing session.
///
/// Reads `metadata_json.permission_mode`. When missing (legacy sessions), infers from the
/// last enter/exit user notice or falls back to `settings_mode`, then persists to metadata.
pub fn resolve_session_permission_mode(
    db: &Database,
    session_id: &str,
    stored_messages: &[StoredMessage],
    settings_mode: &str,
) -> Result<PermissionMode, StateError> {
    if let Some(raw) = db.get_session_permission_mode(session_id)? {
        return PermissionMode::try_from_ipc(&raw).map_err(StateError::Validation);
    }
    let mode = infer_legacy_permission_mode(stored_messages, settings_mode)
        .unwrap_or_else(|| PermissionMode::from_settings_str(settings_mode));
    db.set_session_permission_mode(session_id, mode.label())?;
    Ok(mode)
}

fn infer_legacy_permission_mode(
    stored_messages: &[StoredMessage],
    settings_mode: &str,
) -> Option<PermissionMode> {
    match infer_notice_direction_from_messages(stored_messages)? {
        true => Some(PermissionMode::Unattended),
        false => Some(PermissionMode::from_settings_str(settings_mode)),
    }
}

/// Last permission-mode notice in dialogue: `Some(true)` = enter, `Some(false)` = exit, `None` = none.
fn infer_notice_direction_from_messages(stored: &[StoredMessage]) -> Option<bool> {
    stored
        .iter()
        .rev()
        .filter(|m| m.role == "user")
        .find_map(|m| {
            let content = m.content_json.get("content").and_then(|v| v.as_str())?;
            if content.starts_with(CONTEXT_REFRESH_PREFIX) {
                return None;
            }
            permission_notice_direction(content)
        })
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    fn test_db() -> (TempDir, Database) {
        let tmp = TempDir::new().unwrap();
        let db = Database::open(tmp.path().join("t.db")).unwrap();
        (tmp, db)
    }

    fn stored_user(content: &str) -> StoredMessage {
        StoredMessage {
            id: "m1".into(),
            session_id: "s".into(),
            turn_number: 1,
            sequence: 0,
            role: "user".into(),
            content_json: serde_json::json!({ "content": content }),
            cache_hit_tokens: 0,
            cache_miss_tokens: 0,
            completion_tokens: 0,
            estimated_tokens: None,
            created_at: chrono::Utc::now(),
        }
    }

    #[test]
    fn metadata_takes_priority() {
        let (_tmp, db) = test_db();
        let sid = db.create_session("/p", "m").unwrap();
        db.set_session_permission_mode(&sid, "auto").unwrap();
        let mode = resolve_session_permission_mode(&db, &sid, &[], "normal").unwrap();
        assert_eq!(mode, PermissionMode::Auto);
    }

    #[test]
    fn invalid_metadata_returns_validation_error() {
        let (_tmp, db) = test_db();
        let sid = db.create_session("/p", "m").unwrap();
        db.set_session_permission_mode(&sid, "bogus").unwrap();
        let err = resolve_session_permission_mode(&db, &sid, &[], "normal").unwrap_err();
        assert!(matches!(err, StateError::Validation(_)));
    }

    #[test]
    fn infer_enter_backfills_metadata() {
        use super::super::mode_prompt::PERMISSION_MODE_ENTER_PREFIX;
        let (_tmp, db) = test_db();
        let sid = db.create_session("/p", "m").unwrap();
        let msgs = vec![stored_user(&format!(
            "{PERMISSION_MODE_ENTER_PREFIX}\n\nbody"
        ))];
        let mode = resolve_session_permission_mode(&db, &sid, &msgs, "normal").unwrap();
        assert_eq!(mode, PermissionMode::Unattended);
        assert_eq!(
            db.get_session_permission_mode(&sid).unwrap().as_deref(),
            Some("unattended")
        );
    }

    #[test]
    fn infer_exit_backfills_settings_mode() {
        use super::super::mode_prompt::PERMISSION_MODE_EXIT_PREFIX;
        let (_tmp, db) = test_db();
        let sid = db.create_session("/p", "m").unwrap();
        let msgs = vec![stored_user(&format!(
            "{PERMISSION_MODE_EXIT_PREFIX}\n\nbody"
        ))];
        let mode = resolve_session_permission_mode(&db, &sid, &msgs, "plan").unwrap();
        assert_eq!(mode, PermissionMode::Plan);
        assert_eq!(
            db.get_session_permission_mode(&sid).unwrap().as_deref(),
            Some("plan")
        );
    }

    #[test]
    fn settings_fallback_backfills_metadata() {
        let (_tmp, db) = test_db();
        let sid = db.create_session("/p", "m").unwrap();
        let mode = resolve_session_permission_mode(&db, &sid, &[], "auto").unwrap();
        assert_eq!(mode, PermissionMode::Auto);
        assert_eq!(
            db.get_session_permission_mode(&sid).unwrap().as_deref(),
            Some("auto")
        );
    }
}
