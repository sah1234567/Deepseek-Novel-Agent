//! Plan validation: DAG acyclicity, deps, paths, loop station refs.

use crate::error::{GraphError, GraphResult};
use crate::types::{ArtifactRef, OnReject, PlanGraph, PlanNode};
use std::collections::{HashMap, HashSet, VecDeque};

pub fn parse_plan(json: &str) -> GraphResult<PlanGraph> {
    let plan: PlanGraph = serde_json::from_str(json)?;
    validate_plan(&plan)?;
    Ok(plan)
}

pub fn validate_plan(plan: &PlanGraph) -> GraphResult<()> {
    if plan.nodes.is_empty() {
        return Err(GraphError::Validation("plan has no nodes".into()));
    }
    let ids: HashSet<&str> = plan.nodes.iter().map(|n| n.id.as_str()).collect();
    if ids.len() != plan.nodes.len() {
        return Err(GraphError::Validation("duplicate node ids".into()));
    }
    for node in &plan.nodes {
        validate_node(node, &ids)?;
    }
    detect_cycle(plan)?;
    for lp in &plan.loops {
        for s in &lp.stations {
            if !ids.contains(s.as_str()) {
                return Err(GraphError::Validation(format!(
                    "loop {} station `{s}` not in nodes",
                    lp.id
                )));
            }
        }
        if !lp.stations.iter().any(|s| s == &lp.entry) {
            return Err(GraphError::Validation(format!(
                "loop {} entry `{}` not in stations",
                lp.id, lp.entry
            )));
        }
        if !lp.stations.iter().any(|s| s == &lp.advance_after) {
            return Err(GraphError::Validation(format!(
                "loop {} advance_after `{}` not in stations",
                lp.id, lp.advance_after
            )));
        }
        for r in &lp.on_advance.reopen {
            if !ids.contains(r.as_str()) {
                return Err(GraphError::Validation(format!(
                    "loop {} on_advance.reopen `{r}` not in nodes",
                    lp.id
                )));
            }
        }
    }
    Ok(())
}

fn validate_node(node: &PlanNode, ids: &HashSet<&str>) -> GraphResult<()> {
    for d in &node.deps {
        if !ids.contains(d.as_str()) {
            return Err(GraphError::Validation(format!(
                "node `{}` deps unknown `{d}`",
                node.id
            )));
        }
        if d == &node.id {
            return Err(GraphError::Validation(format!(
                "node `{}` cannot depend on itself",
                node.id
            )));
        }
    }
    if node.spec.is_none() && node.spec_template.is_none() {
        return Err(GraphError::Validation(format!(
            "node `{}` needs spec or spec_template",
            node.id
        )));
    }
    for a in &node.artifacts {
        validate_artifact(a)?;
    }
    if let Some(h) = &node.acceptance.human {
        if h.required {
            let has_primary = node
                .artifacts
                .iter()
                .any(|a| matches!(a.role, crate::types::ArtifactRole::PrimaryDeliverable));
            if !has_primary {
                return Err(GraphError::Validation(format!(
                    "node `{}` human.required needs primary_deliverable artifact",
                    node.id
                )));
            }
        }
        let _ = matches!(h.on_reject, OnReject::Continue | OnReject::Fail);
    }
    for eid in &node.rollback.explicit_ids {
        if !ids.contains(eid.as_str()) {
            return Err(GraphError::Validation(format!(
                "node `{}` rollback.explicit_ids unknown `{eid}`",
                node.id
            )));
        }
    }
    Ok(())
}

fn validate_artifact(a: &ArtifactRef) -> GraphResult<()> {
    let path = a.path.as_deref().or(a.path_template.as_deref());
    let Some(p) = path else {
        return Err(GraphError::Validation(
            "artifact needs path or path_template".into(),
        ));
    };
    if p.starts_with('/') || p.contains("..") || (p.len() >= 2 && p.as_bytes()[1] == b':') {
        return Err(GraphError::Validation(format!(
            "artifact path escapes work root: {p}"
        )));
    }
    Ok(())
}

fn detect_cycle(plan: &PlanGraph) -> GraphResult<()> {
    let mut adj: HashMap<&str, Vec<&str>> = HashMap::new();
    let mut indeg: HashMap<&str, usize> = HashMap::new();
    for n in &plan.nodes {
        indeg.entry(n.id.as_str()).or_insert(0);
        adj.entry(n.id.as_str()).or_default();
    }
    for n in &plan.nodes {
        for d in &n.deps {
            adj.entry(d.as_str()).or_default().push(n.id.as_str());
            *indeg.entry(n.id.as_str()).or_insert(0) += 1;
        }
    }
    let mut q: VecDeque<&str> = indeg
        .iter()
        .filter(|(_, &v)| v == 0)
        .map(|(k, _)| *k)
        .collect();
    let mut seen = 0usize;
    while let Some(u) = q.pop_front() {
        seen += 1;
        if let Some(nexts) = adj.get(u) {
            for v in nexts {
                if let Some(e) = indeg.get_mut(v) {
                    *e -= 1;
                    if *e == 0 {
                        q.push_back(v);
                    }
                }
            }
        }
    }
    if seen != plan.nodes.len() {
        return Err(GraphError::Validation(
            "plan deps contain a cycle (loops must use reopen policy, not deps edges)".into(),
        ));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rejects_cycle() {
        let json = r#"{
          "version":"1",
          "nodes":[
            {"id":"a","title":"A","spec":"x","deps":["b"]},
            {"id":"b","title":"B","spec":"y","deps":["a"]}
          ]
        }"#;
        assert!(parse_plan(json).is_err());
    }

    #[test]
    fn accepts_dag() {
        let json = r#"{
          "version":"1",
          "nodes":[
            {"id":"a","title":"A","spec":"x","deps":[]},
            {"id":"b","title":"B","spec":"y","deps":["a"]}
          ]
        }"#;
        assert!(parse_plan(json).is_ok());
    }

    #[test]
    fn rejects_path_escape() {
        let json = r#"{
          "version":"1",
          "nodes":[{
            "id":"a","title":"A","spec":"x",
            "artifacts":[{"path":"../secret","role":"primary_deliverable"}]
          }]
        }"#;
        assert!(parse_plan(json).is_err());
    }

    #[test]
    fn rejects_empty_nodes() {
        let json = r#"{"version":"1","nodes":[]}"#;
        assert!(parse_plan(json).is_err());
    }

    #[test]
    fn rejects_duplicate_ids() {
        let json = r#"{
          "version":"1",
          "nodes":[
            {"id":"a","title":"A","spec":"x"},
            {"id":"a","title":"A2","spec":"y"}
          ]
        }"#;
        assert!(parse_plan(json).is_err());
    }

    #[test]
    fn rejects_unknown_dep_and_self_dep() {
        assert!(parse_plan(
            r#"{"version":"1","nodes":[{"id":"a","title":"A","spec":"x","deps":["missing"]}]}"#
        )
        .is_err());
        assert!(parse_plan(
            r#"{"version":"1","nodes":[{"id":"a","title":"A","spec":"x","deps":["a"]}]}"#
        )
        .is_err());
    }

    #[test]
    fn rejects_missing_spec() {
        assert!(parse_plan(r#"{"version":"1","nodes":[{"id":"a","title":"A"}]}"#).is_err());
    }

    #[test]
    fn rejects_human_required_without_primary() {
        let json = r#"{
          "version":"1",
          "nodes":[{
            "id":"a","title":"A","spec":"x",
            "acceptance":{"human":{"required":true,"on_reject":"continue"}}
          }]
        }"#;
        assert!(parse_plan(json).is_err());
    }

    #[test]
    fn rejects_bad_loop_refs() {
        let json = r#"{
          "version":"1",
          "nodes":[{"id":"a","title":"A","spec":"x"}],
          "loops":[{
            "id":"L",
            "stations":["a"],
            "entry":"missing",
            "advance_after":"a",
            "cursor":{"chapter":1,"volume":1,"fine_outline_through":0,"round":1},
            "until":{"op":"chapter_gt","value":10},
            "on_advance":{"reopen":["a"]},
            "world_state_board":[]
          }]
        }"#;
        assert!(parse_plan(json).is_err());
    }

    #[test]
    fn rejects_unknown_rollback_explicit_id() {
        let json = r#"{
          "version":"1",
          "nodes":[{
            "id":"a","title":"A","spec":"x",
            "rollback":{"allowed_targets":"none","explicit_ids":["ghost"],"max_regates":3}
          }]
        }"#;
        assert!(parse_plan(json).is_err());
    }

    #[test]
    fn rejects_artifact_without_path() {
        let json = r#"{
          "version":"1",
          "nodes":[{
            "id":"a","title":"A","spec":"x",
            "artifacts":[{"role":"primary_deliverable"}]
          }]
        }"#;
        assert!(parse_plan(json).is_err());
    }
}
