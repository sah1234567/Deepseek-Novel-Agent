use std::path::Path;

use novel_state::Database;

use crate::context::system_prompt::DynamicContext;

/// Frozen static system segments snapshotted at session creation.
#[derive(Debug, Clone)]
pub struct FrozenStaticContext {
    pub agents_md: String,
    pub workspace_path: String,
}

pub fn persist_frozen_system_metadata(
    db: &Database,
    session_id: &str,
    dynamic: &DynamicContext,
) -> Result<(), novel_state::StateError> {
    use crate::context::system_prompt::system_static_sha256;
    let meta = serde_json::json!({
        "system_static_frozen": true,
        "frozen_agents_md": dynamic.agents_md,
        "frozen_workspace_path": dynamic.workspace_path,
        "system_static_sha256": system_static_sha256(dynamic),
        "compaction_count": 0,
    });
    db.set_session_metadata(session_id, &meta)
}

pub fn load_frozen_static_from_metadata(
    db: &Database,
    session_id: &str,
) -> Result<FrozenStaticContext, novel_state::StateError> {
    db.require_frozen_system_metadata(session_id)?;
    let meta = db.get_session_metadata(session_id)?.ok_or_else(|| {
        novel_state::StateError::Validation(format!("session {session_id} missing metadata"))
    })?;
    let agents_md = meta
        .get("frozen_agents_md")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string();
    let workspace_path = meta
        .get("frozen_workspace_path")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string();
    Ok(FrozenStaticContext {
        agents_md,
        workspace_path,
    })
}

/// Rebuild system prompt with frozen AGENTS/Workspace and freshly loaded dynamic sections.
pub fn refresh_system_dynamic_context(
    project_root: &Path,
    session_id: &str,
    db: &Database,
    agent_skills_dir: &Path,
    frozen: &FrozenStaticContext,
) -> DynamicContext {
    let live = super::build_dynamic_context(
        project_root,
        session_id,
        db,
        &frozen.agents_md,
        agent_skills_dir,
    );
    DynamicContext {
        agents_md: frozen.agents_md.clone(),
        workspace_path: frozen.workspace_path.clone(),
        skill_summaries: live.skill_summaries,
        knowledge_index: live.knowledge_index,
        memory: live.memory,
        progress: live.progress,
    }
}
