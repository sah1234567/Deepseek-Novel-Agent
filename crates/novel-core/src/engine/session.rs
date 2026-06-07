use novel_state::Database;
use std::path::PathBuf;

#[derive(Clone)]
pub struct SessionHandle {
    pub id: String,
    pub project_root: PathBuf,
    pub db: Database,
}

impl SessionHandle {
    pub fn create(
        project_root: PathBuf,
        db_path: PathBuf,
        model: &str,
    ) -> Result<Self, novel_state::StateError> {
        let db = Database::open(db_path)?;
        let id = db.create_session(project_root.to_string_lossy().as_ref(), model)?;
        Ok(Self {
            id,
            project_root,
            db,
        })
    }

    pub fn resume(
        project_root: PathBuf,
        db_path: PathBuf,
        session_id: &str,
    ) -> Result<Self, novel_state::StateError> {
        let db = Database::open(db_path)?;
        let session = db
            .get_session(session_id)?
            .ok_or_else(|| novel_state::StateError::SessionNotFound(session_id.into()))?;
        if session.status != "active" {
            db.update_session_status(session_id, "active")?;
        }
        Ok(Self {
            id: session_id.to_string(),
            project_root,
            db,
        })
    }
}
