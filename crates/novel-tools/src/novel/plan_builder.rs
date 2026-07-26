//! PlanBuilder tool — incremental plan-graph construction.
//!
//! Operations: add_node, update_node, remove_node, add_loop, update_loop, remove_loop,
//! set_dep, remove_dep, preview, commit, reset.

use crate::{require_str, Tool, ToolContext, ToolError, ToolOutput};
use async_trait::async_trait;
use novel_graph::{save_plan, validate_plan, GraphTracker, PlanGraph};
use serde_json::{json, Value};

fn load_or_init_draft(ctx: &ToolContext) -> Result<PlanGraph, ToolError> {
    let mut guard = ctx
        .plan_builder_draft
        .lock()
        .map_err(|e| ToolError::Execution(format!("plan_builder lock: {e}")))?;
    if guard.is_none() {
        let existing = novel_graph::load_plan(&ctx.project_root)
            .map_err(|e| ToolError::Execution(e.to_string()))?;
        *guard = Some(existing.unwrap_or(PlanGraph {
            version: "1".into(),
            max_parallel_nodes: 4,
            ..Default::default()
        }));
    }
    guard
        .as_ref()
        .cloned()
        .ok_or_else(|| ToolError::Execution("plan_builder: draft not initialized".into()))
}

fn save_draft(ctx: &ToolContext, plan: PlanGraph) -> Result<(), ToolError> {
    let mut guard = ctx
        .plan_builder_draft
        .lock()
        .map_err(|e| ToolError::Execution(format!("plan_builder lock: {e}")))?;
    *guard = Some(plan);
    Ok(())
}

fn add_node_op(ctx: &ToolContext, input: &Value) -> Result<ToolOutput, ToolError> {
    let mut plan = load_or_init_draft(ctx)?;
    let node_id = require_str(input, "node_id")?.to_string();
    let title = require_str(input, "title")?.to_string();
    let spec = input.get("spec").and_then(|v| v.as_str()).map(String::from);
    let spec_template = input
        .get("spec_template")
        .and_then(|v| v.as_str())
        .map(String::from);
    let deps: Vec<String> = input
        .get("deps")
        .and_then(|v| v.as_array())
        .map(|a| {
            a.iter()
                .filter_map(|v| v.as_str().map(String::from))
                .collect()
        })
        .unwrap_or_default();
    let tags: Vec<String> = input
        .get("tags")
        .and_then(|v| v.as_array())
        .map(|a| {
            a.iter()
                .filter_map(|v| v.as_str().map(String::from))
                .collect()
        })
        .unwrap_or_default();

    // Check for duplicate
    if plan.nodes.iter().any(|n| n.id == node_id) {
        return Err(ToolError::Execution(format!(
            "node '{node_id}' already exists — use update_node to modify"
        )));
    }
    // Check deps exist
    for d in &deps {
        if !plan.nodes.iter().any(|n| &n.id == d) {
            return Err(ToolError::Execution(format!(
                "dependency '{d}' not found — add that node first"
            )));
        }
    }

    let artifacts = if let Some(arr) = input.get("artifacts").and_then(|v| v.as_array()) {
        arr.iter()
            .map(|v| {
                Ok(novel_graph::ArtifactRef {
                    path: v.get("path").and_then(|p| p.as_str()).map(String::from),
                    path_template: v
                        .get("path_template")
                        .and_then(|p| p.as_str())
                        .map(String::from),
                    role: v
                        .get("role")
                        .and_then(|r| r.as_str())
                        .map(|s| match s {
                            "primary_deliverable" => novel_graph::ArtifactRole::PrimaryDeliverable,
                            "input" => novel_graph::ArtifactRole::Input,
                            _ => novel_graph::ArtifactRole::Aux,
                        })
                        .unwrap_or_default(),
                })
            })
            .collect::<Result<Vec<_>, ToolError>>()?
    } else {
        vec![]
    };

    plan.nodes.push(novel_graph::PlanNode {
        id: node_id.clone(),
        title,
        spec,
        spec_template,
        deps,
        tags,
        artifacts,
        ..Default::default()
    });
    save_draft(ctx, plan)?;
    Ok(ToolOutput {
        content: format!("Node '{node_id}' added to draft."),
        is_error: false,
    })
}

fn add_loop_op(ctx: &ToolContext, input: &Value) -> Result<ToolOutput, ToolError> {
    let mut plan = load_or_init_draft(ctx)?;
    let loop_id = require_str(input, "loop_id")?.to_string();
    let stations: Vec<String> = input
        .get("stations")
        .and_then(|v| v.as_array())
        .map(|a| {
            a.iter()
                .filter_map(|v| v.as_str().map(String::from))
                .collect()
        })
        .unwrap_or_default();
    let entry = require_str(input, "entry")?.to_string();
    let advance_after = require_str(input, "advance_after")?.to_string();
    let cursor: novel_graph::Cursor = input
        .get("cursor")
        .map(|v| serde_json::from_value(v.clone()))
        .transpose()
        .map_err(|e| ToolError::Execution(e.to_string()))?
        .unwrap_or_default();
    let advance: novel_graph::AdvanceRule = input
        .get("advance")
        .map(|v| serde_json::from_value(v.clone()))
        .transpose()
        .map_err(|e| ToolError::Execution(e.to_string()))?
        .unwrap_or_default();
    let until: novel_graph::Until = input
        .get("until")
        .map(|v| serde_json::from_value(v.clone()))
        .transpose()
        .map_err(|e| ToolError::Execution(e.to_string()))?
        .unwrap_or(novel_graph::Until::Manual);
    let on_advance: novel_graph::LoopOnAdvance = input
        .get("on_advance")
        .map(|v| serde_json::from_value(v.clone()))
        .transpose()
        .map_err(|e| ToolError::Execution(e.to_string()))?
        .unwrap_or(novel_graph::LoopOnAdvance {
            reopen: stations.clone(),
            clear_node_sessions: true,
            reinject_objectives: true,
            preserve_canon_files: true,
        });

    // Validate references
    for s in &stations {
        if !plan.nodes.iter().any(|n| n.id.as_str() == s.as_str()) {
            return Err(ToolError::Execution(format!(
                "station '{s}' not found in nodes"
            )));
        }
    }
    if !stations.iter().any(|s| s.as_str() == entry) {
        return Err(ToolError::Execution(format!(
            "entry '{entry}' must be in stations list"
        )));
    }
    if !stations.iter().any(|s| s.as_str() == advance_after) {
        return Err(ToolError::Execution(format!(
            "advance_after '{advance_after}' must be in stations list"
        )));
    }

    plan.loops.push(novel_graph::WorkflowLoop {
        id: loop_id.clone(),
        stations,
        entry,
        advance_after,
        cursor,
        advance,
        until,
        on_advance,
        world_state_board: vec![],
    });
    save_draft(ctx, plan)?;
    Ok(ToolOutput {
        content: format!("Loop '{loop_id}' added to draft."),
        is_error: false,
    })
}

fn set_dep_op(ctx: &ToolContext, input: &Value) -> Result<ToolOutput, ToolError> {
    let mut plan = load_or_init_draft(ctx)?;
    let node_id = require_str(input, "node_id")?.to_string();
    let depends_on = require_str(input, "depends_on")?.to_string();

    // Validate both nodes exist (immutable borrow first)
    if !plan.nodes.iter().any(|n| n.id == node_id) {
        return Err(ToolError::Execution(format!("node '{node_id}' not found")));
    }
    if !plan.nodes.iter().any(|n| n.id == depends_on) {
        return Err(ToolError::Execution(format!(
            "dependency '{depends_on}' not found"
        )));
    }

    // Push dep (mutable borrow scoped)
    {
        let node = plan
            .nodes
            .iter_mut()
            .find(|n| n.id == node_id)
            .ok_or_else(|| ToolError::Execution("node vanished during set_dep".into()))?;
        if node.deps.iter().any(|d| d == &depends_on) {
            return Err(ToolError::Execution(format!(
                "'{node_id}' already depends on '{depends_on}'"
            )));
        }
        node.deps.push(depends_on.clone());
    }

    // Cycle check: re-validate (immutable borrow after mutable dropped)
    if validate_plan(&plan).is_err() {
        if let Some(node) = plan.nodes.iter_mut().find(|n| n.id == node_id) {
            node.deps.retain(|d| d != &depends_on);
        }
        return Err(ToolError::Execution(format!(
            "adding dep '{depends_on}' → '{node_id}' would create a cycle"
        )));
    }

    save_draft(ctx, plan)?;
    Ok(ToolOutput {
        content: format!("Dependency '{depends_on}' → '{node_id}' added."),
        is_error: false,
    })
}

fn remove_node_op(ctx: &ToolContext, input: &Value) -> Result<ToolOutput, ToolError> {
    let mut plan = load_or_init_draft(ctx)?;
    let node_id = require_str(input, "node_id")?;
    if !plan.nodes.iter().any(|n| n.id == node_id) {
        return Err(ToolError::Execution(format!("node '{node_id}' not found")));
    }
    plan.nodes.retain(|n| n.id.as_str() != node_id);
    // Clean up deps referencing this node
    for n in &mut plan.nodes {
        n.deps.retain(|d| d.as_str() != node_id);
    }
    // Clean up loop references
    for lp in &mut plan.loops {
        lp.stations.retain(|s| s.as_str() != node_id);
        lp.on_advance.reopen.retain(|s| s.as_str() != node_id);
    }
    save_draft(ctx, plan)?;
    Ok(ToolOutput {
        content: format!("Node '{node_id}' removed."),
        is_error: false,
    })
}

fn update_node_op(ctx: &ToolContext, input: &Value) -> Result<ToolOutput, ToolError> {
    let mut plan = load_or_init_draft(ctx)?;
    let node_id = require_str(input, "node_id")?.to_string();
    let node = plan
        .nodes
        .iter_mut()
        .find(|n| n.id == node_id)
        .ok_or_else(|| ToolError::Execution(format!("node '{node_id}' not found")))?;
    if let Some(title) = input.get("title").and_then(|v| v.as_str()) {
        node.title = title.to_string();
    }
    if let Some(spec) = input.get("spec").and_then(|v| v.as_str()) {
        node.spec = Some(spec.to_string());
    }
    if let Some(t) = input.get("spec_template").and_then(|v| v.as_str()) {
        node.spec_template = Some(t.to_string());
    }
    if let Some(arr) = input.get("tags").and_then(|v| v.as_array()) {
        node.tags = arr
            .iter()
            .filter_map(|v| v.as_str().map(String::from))
            .collect();
    }
    if let Some(arr) = input.get("deps").and_then(|v| v.as_array()) {
        node.deps = arr
            .iter()
            .filter_map(|v| v.as_str().map(String::from))
            .collect();
    }
    save_draft(ctx, plan)?;
    Ok(ToolOutput {
        content: format!("Node '{node_id}' updated."),
        is_error: false,
    })
}

fn update_loop_op(ctx: &ToolContext, input: &Value) -> Result<ToolOutput, ToolError> {
    let mut plan = load_or_init_draft(ctx)?;
    let loop_id = require_str(input, "loop_id")?.to_string();
    let lp = plan
        .loops
        .iter_mut()
        .find(|l| l.id == loop_id)
        .ok_or_else(|| ToolError::Execution(format!("loop '{loop_id}' not found")))?;
    if let Some(v) = input
        .get("cursor")
        .map(|v| serde_json::from_value(v.clone()))
        .transpose()
        .map_err(|e| ToolError::Execution(e.to_string()))?
    {
        lp.cursor = v;
    }
    if let Some(v) = input
        .get("advance")
        .map(|v| serde_json::from_value(v.clone()))
        .transpose()
        .map_err(|e| ToolError::Execution(e.to_string()))?
    {
        lp.advance = v;
    }
    if let Some(v) = input
        .get("until")
        .map(|v| serde_json::from_value(v.clone()))
        .transpose()
        .map_err(|e| ToolError::Execution(e.to_string()))?
    {
        lp.until = v;
    }
    save_draft(ctx, plan)?;
    Ok(ToolOutput {
        content: format!("Loop '{loop_id}' updated."),
        is_error: false,
    })
}

fn preview_op(ctx: &ToolContext) -> Result<ToolOutput, ToolError> {
    let plan = load_or_init_draft(ctx)?;
    let mut lines = vec!["## Plan Draft Preview".into(), String::new()];

    lines.push("### Nodes".into());
    if plan.nodes.is_empty() {
        lines.push("(none)".into());
    } else {
        for n in &plan.nodes {
            let deps_str = if n.deps.is_empty() {
                String::new()
            } else {
                format!(" ← {}", n.deps.join(", "))
            };
            let tags_str = if n.tags.is_empty() {
                String::new()
            } else {
                format!(" [{}]", n.tags.join(", "))
            };
            lines.push(format!(
                "- **{}**{}: {}{}",
                n.id, tags_str, n.title, deps_str
            ));
        }
    }

    lines.push(String::new());
    lines.push("### Loops".into());
    if plan.loops.is_empty() {
        lines.push("(none)".into());
    } else {
        for lp in &plan.loops {
            lines.push(format!(
                "- **{}**: {} → {} (advance on: {})",
                lp.id,
                lp.stations.join(" → "),
                lp.entry,
                lp.advance_after
            ));
        }
    }

    Ok(ToolOutput {
        content: lines.join("\n"),
        is_error: false,
    })
}

fn commit_op(ctx: &ToolContext) -> Result<ToolOutput, ToolError> {
    let plan = load_or_init_draft(ctx)?;
    validate_plan(&plan).map_err(|e| ToolError::Execution(e.to_string()))?;
    save_plan(&ctx.project_root, &plan).map_err(|e| ToolError::Execution(e.to_string()))?;
    let t = GraphTracker::new(plan);
    t.save(&ctx.project_root)
        .map_err(|e| ToolError::Execution(e.to_string()))?;
    if let Some(cb) = &ctx.on_graph_plan_committed {
        cb();
    }
    // Clear draft after successful commit
    let mut guard = ctx
        .plan_builder_draft
        .lock()
        .map_err(|e| ToolError::Execution(format!("plan_builder lock: {e}")))?;
    *guard = None;
    Ok(ToolOutput {
        content: "Plan committed successfully. Graph is now active.".into(),
        is_error: false,
    })
}

fn reset_op(ctx: &ToolContext) -> Result<ToolOutput, ToolError> {
    let mut guard = ctx
        .plan_builder_draft
        .lock()
        .map_err(|e| ToolError::Execution(format!("plan_builder lock: {e}")))?;
    *guard = None;
    Ok(ToolOutput {
        content: "Draft discarded. Reload from disk on next operation.".into(),
        is_error: false,
    })
}

pub struct PlanBuilderTool;

#[async_trait]
impl Tool for PlanBuilderTool {
    fn name(&self) -> &str {
        "PlanBuilder"
    }
    fn description(&self) -> &str {
        "Incrementally build a plan graph. Operations: add_node, update_node, remove_node, add_loop, remove_loop, set_dep, remove_dep, preview, commit, reset. Use preview before commit to review."
    }
    fn input_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "operation": {
                    "type": "string",
                    "enum": ["add_node", "update_node", "remove_node",
                             "add_loop", "update_loop", "remove_loop",
                             "set_dep", "remove_dep",
                             "preview", "commit", "reset"]
                },
                "node_id": { "type": "string", "description": "Node identifier" },
                "title": { "type": "string", "description": "Human-readable node title" },
                "spec": { "type": "string", "description": "Static specification (or use spec_template)" },
                "spec_template": { "type": "string", "description": "Template with {{cursor.X}} variables" },
                "deps": { "type": "array", "items": { "type": "string" }, "description": "Dependency node IDs" },
                "tags": { "type": "array", "items": { "type": "string" }, "description": "Domain tags" },
                "depends_on": { "type": "string", "description": "Node ID to depend on (for set_dep/remove_dep)" },
                "loop_id": { "type": "string", "description": "Loop identifier" },
                "stations": { "type": "array", "items": { "type": "string" }, "description": "Station node IDs in order" },
                "entry": { "type": "string", "description": "First station to activate" },
                "advance_after": { "type": "string", "description": "Station that triggers advance" },
                "cursor": { "type": "object", "description": "Initial cursor counters and tags" },
                "advance": { "type": "object", "description": "AdvanceRule: increment, step, side_effects" },
                "until": { "type": "object", "description": "Until condition (CounterGt/CounterGe/CounterLt/CounterEq/Manual)" },
                "on_advance": { "type": "object", "description": "LoopOnAdvance settings" }
            },
            "required": ["operation"]
        })
    }
    fn is_read_only(&self) -> bool {
        false
    }

    async fn call(&self, input: Value, ctx: &ToolContext) -> Result<ToolOutput, ToolError> {
        let op = require_str(&input, "operation")?;
        dispatch_plan_builder_op(op.as_str(), &input, ctx)
    }
}

fn remove_loop_op(ctx: &ToolContext, input: &Value) -> Result<ToolOutput, ToolError> {
    let mut plan = load_or_init_draft(ctx)?;
    let loop_id = require_str(input, "loop_id")?;
    plan.loops.retain(|l| l.id != loop_id);
    save_draft(ctx, plan)?;
    Ok(ToolOutput {
        content: format!("Loop '{loop_id}' removed."),
        is_error: false,
    })
}

fn remove_dep_op(ctx: &ToolContext, input: &Value) -> Result<ToolOutput, ToolError> {
    let mut plan = load_or_init_draft(ctx)?;
    let node_id = require_str(input, "node_id")?;
    let depends_on = require_str(input, "depends_on")?;
    let node = plan
        .nodes
        .iter_mut()
        .find(|n| n.id == node_id)
        .ok_or_else(|| ToolError::Execution(format!("node '{node_id}' not found")))?;
    node.deps.retain(|d| d.as_str() != depends_on);
    save_draft(ctx, plan)?;
    Ok(ToolOutput {
        content: format!("Dependency '{depends_on}' removed from '{node_id}'."),
        is_error: false,
    })
}

fn dispatch_plan_builder_op(
    op: &str,
    input: &Value,
    ctx: &ToolContext,
) -> Result<ToolOutput, ToolError> {
    match op {
        "add_node" => add_node_op(ctx, input),
        "update_node" => update_node_op(ctx, input),
        "remove_node" => remove_node_op(ctx, input),
        "add_loop" => add_loop_op(ctx, input),
        "update_loop" => update_loop_op(ctx, input),
        "remove_loop" => remove_loop_op(ctx, input),
        "set_dep" => set_dep_op(ctx, input),
        "remove_dep" => remove_dep_op(ctx, input),
        "preview" => preview_op(ctx),
        "commit" => commit_op(ctx),
        "reset" => reset_op(ctx),
        _ => Err(ToolError::Execution(format!(
            "unknown operation: {op}. Use: add_node, update_node, remove_node, add_loop, update_loop, remove_loop, set_dep, remove_dep, preview, commit, reset"
        ))),
    }
}

#[cfg(test)]
mod tests {
    //! Boundary matrix (before writing cases):
    //! - draft: empty init / load existing disk plan / reset clears / commit clears
    //! - add_node: happy; duplicate; missing dep; artifact roles (primary/input/aux); tags+deps
    //! - update/remove_node: missing id; remove cleans deps + loop stations/reopen
    //! - add_loop: missing station / entry∉stations / advance_after∉stations; bad JSON fields
    //! - update/remove_loop: missing loop; update cursor/advance/until
    //! - set_dep: missing ends; duplicate dep; cycle rollback
    //! - remove_dep: missing node
    //! - preview: empty vs populated
    //! - commit: invalid empty plan fails; valid persists + callback
    //! - unknown operation

    use super::*;
    use crate::PermissionMode;
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::sync::Arc;
    use tempfile::TempDir;

    fn ctx(tmp: &TempDir) -> ToolContext {
        ToolContext {
            permission_mode: PermissionMode::Auto,
            project_root: tmp.path().to_path_buf(),
            ..ToolContext::new(tmp.path().to_path_buf())
        }
    }

    async fn run(ctx: &ToolContext, input: Value) -> Result<ToolOutput, ToolError> {
        PlanBuilderTool.call(input, ctx).await
    }

    #[tokio::test]
    async fn preview_empty_draft_then_reset() {
        let tmp = TempDir::new().unwrap();
        let c = ctx(&tmp);
        let out = run(&c, json!({"operation": "preview"})).await.unwrap();
        assert!(out.content.contains("(none)"));
        run(&c, json!({"operation": "reset"})).await.unwrap();
        assert!(c.plan_builder_draft.lock().unwrap().is_none());
    }

    #[tokio::test]
    async fn add_node_happy_and_duplicate_and_missing_dep() {
        let tmp = TempDir::new().unwrap();
        let c = ctx(&tmp);
        run(
            &c,
            json!({
                "operation": "add_node",
                "node_id": "a",
                "title": "A",
                "spec": "do a",
                "tags": ["gate"],
                "artifacts": [
                    {"path": "out/a.md", "role": "primary_deliverable"},
                    {"path": "in/x.md", "role": "input"},
                    {"path_template": "aux/{{cursor.n}}.md", "role": "other"}
                ]
            }),
        )
        .await
        .unwrap();
        let err = run(
            &c,
            json!({"operation": "add_node", "node_id": "a", "title": "dup", "spec": "x"}),
        )
        .await
        .unwrap_err();
        assert!(err.to_string().contains("already exists"));
        let err = run(
            &c,
            json!({
                "operation": "add_node",
                "node_id": "b",
                "title": "B",
                "spec": "b",
                "deps": ["missing"]
            }),
        )
        .await
        .unwrap_err();
        assert!(err.to_string().contains("dependency 'missing'"));
    }

    #[tokio::test]
    async fn update_remove_node_and_dep_cycle() {
        let tmp = TempDir::new().unwrap();
        let c = ctx(&tmp);
        for (id, title) in [("a", "A"), ("b", "B"), ("c", "C")] {
            run(
                &c,
                json!({"operation": "add_node", "node_id": id, "title": title, "spec": "s"}),
            )
            .await
            .unwrap();
        }
        run(
            &c,
            json!({
                "operation": "update_node",
                "node_id": "b",
                "title": "B2",
                "spec": "new",
                "spec_template": "t-{{cursor.n}}",
                "tags": ["t"],
                "deps": ["a"]
            }),
        )
        .await
        .unwrap();
        assert!(run(
            &c,
            json!({"operation": "update_node", "node_id": "ghost", "title": "x"})
        )
        .await
        .unwrap_err()
        .to_string()
        .contains("not found"));

        run(
            &c,
            json!({"operation": "set_dep", "node_id": "c", "depends_on": "b"}),
        )
        .await
        .unwrap();
        let dup = run(
            &c,
            json!({"operation": "set_dep", "node_id": "c", "depends_on": "b"}),
        )
        .await
        .unwrap_err();
        assert!(dup.to_string().contains("already depends"));
        let cycle = run(
            &c,
            json!({"operation": "set_dep", "node_id": "a", "depends_on": "c"}),
        )
        .await
        .unwrap_err();
        assert!(cycle.to_string().contains("cycle"));
        assert!(run(
            &c,
            json!({"operation": "set_dep", "node_id": "ghost", "depends_on": "a"})
        )
        .await
        .unwrap_err()
        .to_string()
        .contains("not found"));
        assert!(run(
            &c,
            json!({"operation": "set_dep", "node_id": "a", "depends_on": "ghost"})
        )
        .await
        .unwrap_err()
        .to_string()
        .contains("not found"));

        run(
            &c,
            json!({"operation": "remove_dep", "node_id": "c", "depends_on": "b"}),
        )
        .await
        .unwrap();
        assert!(run(
            &c,
            json!({"operation": "remove_dep", "node_id": "ghost", "depends_on": "a"})
        )
        .await
        .unwrap_err()
        .to_string()
        .contains("not found"));

        run(&c, json!({"operation": "remove_node", "node_id": "b"}))
            .await
            .unwrap();
        assert!(run(&c, json!({"operation": "remove_node", "node_id": "b"}))
            .await
            .unwrap_err()
            .to_string()
            .contains("not found"));
    }

    #[tokio::test]
    async fn loop_ops_boundaries_and_commit() {
        let tmp = TempDir::new().unwrap();
        let committed = Arc::new(AtomicBool::new(false));
        let flag = Arc::clone(&committed);
        let mut c = ctx(&tmp);
        c.on_graph_plan_committed = Some(Arc::new(move || {
            flag.store(true, Ordering::SeqCst);
        }));

        run(
            &c,
            json!({
                "operation": "add_node",
                "node_id": "write",
                "title": "Write",
                "spec": "write chapter",
                "artifacts": [{"path": "chapters/001.md", "role": "primary_deliverable"}]
            }),
        )
        .await
        .unwrap();
        run(
            &c,
            json!({
                "operation": "add_node",
                "node_id": "sync",
                "title": "Sync",
                "spec": "sync",
                "deps": ["write"],
                "artifacts": [{"path": "knowledge/board.md", "role": "primary_deliverable"}]
            }),
        )
        .await
        .unwrap();

        assert!(run(
            &c,
            json!({
                "operation": "add_loop",
                "loop_id": "L",
                "stations": ["write", "ghost"],
                "entry": "write",
                "advance_after": "sync"
            })
        )
        .await
        .unwrap_err()
        .to_string()
        .contains("station"));
        assert!(run(
            &c,
            json!({
                "operation": "add_loop",
                "loop_id": "L",
                "stations": ["write", "sync"],
                "entry": "ghost",
                "advance_after": "sync"
            })
        )
        .await
        .unwrap_err()
        .to_string()
        .contains("entry"));
        assert!(run(
            &c,
            json!({
                "operation": "add_loop",
                "loop_id": "L",
                "stations": ["write", "sync"],
                "entry": "write",
                "advance_after": "ghost"
            })
        )
        .await
        .unwrap_err()
        .to_string()
        .contains("advance_after"));

        run(
            &c,
            json!({
                "operation": "add_loop",
                "loop_id": "body",
                "stations": ["write", "sync"],
                "entry": "write",
                "advance_after": "sync",
                "cursor": {"counters": {"chapter": 1}},
                "advance": {"increment": "chapter", "step": 1},
                "until": {"type": "counter_gt", "counter": "chapter", "value": 3},
                "on_advance": {
                    "reopen": ["write", "sync"],
                    "clear_node_sessions": true,
                    "reinject_objectives": true,
                    "preserve_canon_files": true
                }
            }),
        )
        .await
        .unwrap();

        run(
            &c,
            json!({
                "operation": "update_loop",
                "loop_id": "body",
                "cursor": {"counters": {"chapter": 2}},
                "advance": {"increment": "chapter", "step": 1},
                "until": {"type": "manual"}
            }),
        )
        .await
        .unwrap();
        assert!(run(
            &c,
            json!({"operation": "update_loop", "loop_id": "missing", "until": {"type": "manual"}})
        )
        .await
        .unwrap_err()
        .to_string()
        .contains("not found"));

        let preview = run(&c, json!({"operation": "preview"})).await.unwrap();
        assert!(preview.content.contains("write"));
        assert!(preview.content.contains("body"));

        // remove_node cleans loop stations
        run(
            &c,
            json!({
                "operation": "add_node",
                "node_id": "tmp",
                "title": "T",
                "spec": "t"
            }),
        )
        .await
        .unwrap();
        // expand stations via update_loop isn't supported for stations; remove tmp after adding to stations through draft mutate:
        {
            let mut g = c.plan_builder_draft.lock().unwrap();
            if let Some(p) = g.as_mut() {
                p.loops[0].stations.push("tmp".into());
                p.loops[0].on_advance.reopen.push("tmp".into());
            }
        }
        run(&c, json!({"operation": "remove_node", "node_id": "tmp"}))
            .await
            .unwrap();
        {
            let g = c.plan_builder_draft.lock().unwrap();
            let lp = &g.as_ref().unwrap().loops[0];
            assert!(!lp.stations.iter().any(|s| s == "tmp"));
            assert!(!lp.on_advance.reopen.iter().any(|s| s == "tmp"));
        }

        run(&c, json!({"operation": "remove_loop", "loop_id": "body"}))
            .await
            .unwrap();
        // re-add loop for commit
        run(
            &c,
            json!({
                "operation": "add_loop",
                "loop_id": "body",
                "stations": ["write", "sync"],
                "entry": "write",
                "advance_after": "sync",
                "cursor": {"counters": {"chapter": 1}},
                "advance": {"increment": "chapter", "step": 1},
                "until": {"type": "manual"}
            }),
        )
        .await
        .unwrap();

        let out = run(&c, json!({"operation": "commit"})).await.unwrap();
        assert!(out.content.contains("committed"));
        assert!(committed.load(Ordering::SeqCst));
        assert!(c.plan_builder_draft.lock().unwrap().is_none());
        assert!(novel_graph::plan_exists(tmp.path()));
    }

    #[tokio::test]
    async fn commit_rejects_empty_and_unknown_op() {
        let tmp = TempDir::new().unwrap();
        let c = ctx(&tmp);
        let err = run(&c, json!({"operation": "commit"})).await.unwrap_err();
        assert!(err.to_string().contains("no nodes") || err.to_string().contains("Validation"));
        let err = run(&c, json!({"operation": "nope"})).await.unwrap_err();
        assert!(err.to_string().contains("unknown operation"));
        assert_eq!(PlanBuilderTool.name(), "PlanBuilder");
        assert!(!PlanBuilderTool.is_read_only());
        assert!(PlanBuilderTool.input_schema().get("properties").is_some());
    }

    #[tokio::test]
    async fn loads_existing_plan_from_disk_into_draft() {
        let tmp = TempDir::new().unwrap();
        let plan = novel_graph::PlanGraph {
            version: "1".into(),
            max_parallel_nodes: 4,
            nodes: vec![novel_graph::PlanNode {
                id: "seed".into(),
                title: "Seed".into(),
                spec: Some("s".into()),
                ..Default::default()
            }],
            ..Default::default()
        };
        novel_graph::save_plan(tmp.path(), &plan).unwrap();
        let c = ctx(&tmp);
        let out = run(&c, json!({"operation": "preview"})).await.unwrap();
        assert!(out.content.contains("seed"));
    }
}
