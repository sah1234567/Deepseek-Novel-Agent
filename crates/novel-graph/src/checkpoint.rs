//! Loop checkpoint snapshots under `knowledge/meta/checkpoints/`.

use crate::error::GraphResult;
use crate::types::Cursor;
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::{Path, PathBuf};

pub const CHECKPOINTS_DIR_REL: &str = "knowledge/meta/checkpoints";

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct GraphCheckpoint {
    pub loop_id: String,
    pub cursor: Cursor,
    pub frozen_at: String,
    pub artifact_paths: Vec<String>,
}

pub fn checkpoint_path(work_root: &Path, loop_id: &str) -> PathBuf {
    work_root
        .join(CHECKPOINTS_DIR_REL)
        .join(format!("{loop_id}.json"))
}

pub fn save_checkpoint(work_root: &Path, cp: &GraphCheckpoint) -> GraphResult<()> {
    let p = checkpoint_path(work_root, &cp.loop_id);
    if let Some(parent) = p.parent() {
        fs::create_dir_all(parent)?;
    }
    fs::write(p, serde_json::to_string_pretty(cp)?)?;
    Ok(())
}

pub fn load_checkpoint(work_root: &Path, loop_id: &str) -> GraphResult<Option<GraphCheckpoint>> {
    let p = checkpoint_path(work_root, loop_id);
    if !p.exists() {
        return Ok(None);
    }
    Ok(Some(serde_json::from_str(&fs::read_to_string(p)?)?))
}
