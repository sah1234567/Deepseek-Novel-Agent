//! Ensure plan + graph-state exist under a work root.

use crate::error::GraphResult;
use crate::persist::{load_plan, plan_exists, save_plan, save_state, state_path};
use crate::tracker::GraphTracker;
use crate::{default_plan, default_plan_json, GraphState};
use std::fs;
use std::path::Path;

fn ensure_plan_on_disk(work_root: &Path) -> GraphResult<()> {
    if !plan_exists(work_root) {
        save_plan(work_root, &default_plan()?)?;
        return Ok(());
    }
    // File exists — must be parseable. Do NOT silently overwrite a corrupt / invalid plan.
    let plan = load_plan(work_root)?;
    match plan {
        Some(_) => Ok(()),
        None => Err(crate::error::GraphError::Validation(
            "plan-graph.json exists but could not be parsed; fix or remove it manually".into(),
        )),
    }
}

pub fn ensure_graph_initialized(work_root: &Path) -> GraphResult<GraphTracker> {
    ensure_plan_on_disk(work_root)?;
    let state_missing = !state_path(work_root).exists();
    if let Some(t) = GraphTracker::load(work_root)? {
        if state_missing {
            save_state(work_root, &t.state)?;
        }
        return Ok(t);
    }
    let plan = load_plan(work_root)?.unwrap_or(default_plan()?);
    let t = GraphTracker::new(plan);
    save_state(work_root, &t.state)?;
    Ok(t)
}

/// Copy bundled default plan JSON bytes into work if missing.
/// Writes an empty skeleton — no nodes or loops. The LLM builds the workflow via PlanBuilder.
pub fn write_default_plan_file(work_root: &Path) -> GraphResult<()> {
    let p = work_root.join(crate::types::PLAN_GRAPH_REL);
    if p.exists() {
        return Ok(());
    }
    if let Some(parent) = p.parent() {
        fs::create_dir_all(parent)?;
    }
    fs::write(p, default_plan_json())?;
    // Empty skeleton has no nodes — Trcker::new would fail validation.
    // Just write an empty state so graph-state.json exists.
    save_state(work_root, &GraphState::default())?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn corrupt_plan_returns_error_not_overwrite() {
        let tmp = TempDir::new().unwrap();
        let p = tmp.path().join("knowledge/meta");
        fs::create_dir_all(&p).unwrap();
        fs::write(p.join("plan-graph.json"), "{not json").unwrap();
        let err = ensure_graph_initialized(tmp.path()).unwrap_err();
        assert!(
            format!("{err}").contains("plan-graph.json")
                || format!("{err}").contains("parse")
                || format!("{err}").contains("json"),
            "expected parse/validation error, got: {err}"
        );
        // File must not have been overwritten.
        let raw = fs::read_to_string(p.join("plan-graph.json")).unwrap();
        assert!(
            raw.trim() == "{not json",
            "plan was overwritten; content: {raw:?}"
        );
    }

    #[test]
    fn write_default_plan_noop_when_exists() {
        let tmp = TempDir::new().unwrap();
        write_default_plan_file(tmp.path()).unwrap();
        let plan_path = tmp.path().join("knowledge/meta/plan-graph.json");
        let mtime = fs::metadata(&plan_path).unwrap().modified().unwrap();
        write_default_plan_file(tmp.path()).unwrap();
        let after = fs::metadata(&plan_path).unwrap().modified().unwrap();
        assert_eq!(mtime, after);
    }
}
