//! GraphTracker: status transitions, ready recompute, parallel start, handoff, loop advance.

use crate::error::{GraphError, GraphResult};
use crate::template::render_template;
use crate::types::{
    ArtifactRole, CounterOp, FileTouch, GraphLoopRuntime, GraphNodeRuntime, GraphSettings,
    GraphState, LoopPhase, NodeHandoff, NodeStatus, OnReject, PlanGraph, PlanNode, Until,
};
use chrono::Utc;
use serde_json::json;
use std::collections::{HashMap, HashSet};
use std::path::Path;

#[derive(Debug)]
pub struct GraphTracker {
    pub plan: PlanGraph,
    pub state: GraphState,
}

impl GraphTracker {
    pub fn new(plan: PlanGraph) -> Self {
        let mut state = GraphState {
            plan_version: plan.version.clone(),
            settings: GraphSettings {
                enforce_gates: plan.enforce_gates,
                max_parallel_nodes: plan.max_parallel_nodes,
                auto_start_ready: plan.auto_start_ready,
                settings: plan.settings.clone(),
            },
            ..GraphState::default()
        };
        let loop_of: HashMap<String, String> = plan
            .loops
            .iter()
            .flat_map(|lp| {
                lp.stations
                    .iter()
                    .map(|s| (s.clone(), lp.id.clone()))
                    .collect::<Vec<_>>()
            })
            .collect();
        for n in &plan.nodes {
            state.nodes.insert(
                n.id.clone(),
                GraphNodeRuntime {
                    status: NodeStatus::Waiting,
                    loop_id: loop_of.get(&n.id).cloned(),
                    ..GraphNodeRuntime::default()
                },
            );
        }
        for lp in &plan.loops {
            state.loops.insert(
                lp.id.clone(),
                GraphLoopRuntime {
                    cursor: lp.cursor.clone(),
                    phase: LoopPhase::Idle,
                    ..GraphLoopRuntime::default()
                },
            );
        }
        let mut t = Self { plan, state };
        t.recompute_ready();
        t
    }

    pub fn load(work_root: &Path) -> GraphResult<Option<Self>> {
        let Some(plan) = crate::persist::load_plan(work_root)? else {
            return Ok(None);
        };
        let state_missing = !crate::persist::state_path(work_root).exists();
        let mut state = crate::persist::load_state(work_root)?;
        if state_missing || state.plan_version.is_empty() {
            state.plan_version = plan.version.clone();
            state.settings = GraphSettings {
                enforce_gates: plan.enforce_gates,
                max_parallel_nodes: plan.max_parallel_nodes,
                auto_start_ready: plan.auto_start_ready,
                settings: plan.settings.clone(),
            };
        }
        for n in &plan.nodes {
            state.nodes.entry(n.id.clone()).or_insert_with(|| {
                let loop_id = plan
                    .loops
                    .iter()
                    .find(|lp| lp.stations.iter().any(|s| s == &n.id))
                    .map(|lp| lp.id.clone());
                GraphNodeRuntime {
                    status: NodeStatus::Waiting,
                    loop_id,
                    ..GraphNodeRuntime::default()
                }
            });
        }
        for lp in &plan.loops {
            state
                .loops
                .entry(lp.id.clone())
                .or_insert_with(|| GraphLoopRuntime {
                    cursor: lp.cursor.clone(),
                    phase: LoopPhase::Idle,
                    ..GraphLoopRuntime::default()
                });
        }
        let mut t = Self { plan, state };
        t.recompute_ready();
        Ok(Some(t))
    }

    pub fn save(&self, work_root: &Path) -> GraphResult<()> {
        crate::persist::save_state(work_root, &self.state)
    }

    pub fn node_plan(&self, id: &str) -> GraphResult<&PlanNode> {
        self.plan
            .nodes
            .iter()
            .find(|n| n.id == id)
            .ok_or_else(|| GraphError::NodeNotFound(id.into()))
    }

    pub fn recompute_ready(&mut self) {
        let achieved: HashSet<String> = self
            .state
            .nodes
            .iter()
            .filter(|(_, r)| r.status == NodeStatus::Achieved)
            .map(|(id, _)| id.clone())
            .collect();
        for n in &self.plan.nodes {
            let Some(rt) = self.state.nodes.get_mut(&n.id) else {
                continue;
            };
            if !matches!(rt.status, NodeStatus::Waiting | NodeStatus::Ready) {
                continue;
            }
            let ok = n.deps.iter().all(|d| achieved.contains(d));
            rt.status = if ok {
                NodeStatus::Ready
            } else {
                NodeStatus::Waiting
            };
        }
    }

    pub fn set_focus(&mut self, node_id: Option<String>) -> GraphResult<()> {
        if let Some(ref id) = node_id {
            if !self.state.nodes.contains_key(id) {
                return Err(GraphError::NodeNotFound(id.clone()));
            }
        }
        self.state.focused_node_id = node_id;
        Ok(())
    }

    pub fn writable_paths(&self, node_id: &str) -> GraphResult<Vec<String>> {
        let plan_n = self.node_plan(node_id)?;
        let cursor = self
            .state
            .nodes
            .get(node_id)
            .and_then(|r| r.loop_id.as_ref())
            .and_then(|lid| self.state.loops.get(lid))
            .map(|l| l.cursor.clone())
            .unwrap_or_default();
        let extras = HashMap::new();
        let mut out = Vec::new();
        for a in &plan_n.artifacts {
            if matches!(a.role, ArtifactRole::Input) {
                continue;
            }
            if let Some(p) = &a.path {
                out.push(p.clone());
            } else if let Some(t) = &a.path_template {
                out.push(render_template(t, &cursor, &extras));
            }
        }
        Ok(out)
    }

    fn write_overlap(&self, node_id: &str) -> GraphResult<bool> {
        let mine: HashSet<String> = self.writable_paths(node_id)?.into_iter().collect();
        for rid in &self.state.running_node_ids {
            if rid == node_id {
                continue;
            }
            let other: HashSet<String> = self.writable_paths(rid)?.into_iter().collect();
            if mine.intersection(&other).next().is_some() {
                return Ok(true);
            }
        }
        Ok(false)
    }

    pub fn start_node(&mut self, node_id: &str) -> GraphResult<()> {
        let rt = self
            .state
            .nodes
            .get(node_id)
            .ok_or_else(|| GraphError::NodeNotFound(node_id.into()))?;
        if rt.status != NodeStatus::Ready && rt.status != NodeStatus::Running {
            return Err(GraphError::IllegalTransition(format!(
                "cannot start `{node_id}` from {:?}",
                rt.status
            )));
        }
        if self.state.running_node_ids.len() >= self.state.settings.max_parallel_nodes
            && !self.state.running_node_ids.iter().any(|id| id == node_id)
        {
            return Err(GraphError::IllegalTransition(format!(
                "max_parallel_nodes={}",
                self.state.settings.max_parallel_nodes
            )));
        }
        if self.write_overlap(node_id)? {
            return Err(GraphError::IllegalTransition(
                "parallel_blocked: write_overlap".into(),
            ));
        }
        let effective = self.render_effective_spec(node_id)?;
        let rt = self
            .state
            .nodes
            .get_mut(node_id)
            .ok_or_else(|| GraphError::NodeNotFound(node_id.into()))?;
        rt.status = NodeStatus::Running;
        rt.effective_spec = Some(effective);
        rt.files_touched_journal.clear();
        rt.pending_summary = None;
        if !self.state.running_node_ids.iter().any(|id| id == node_id) {
            self.state.running_node_ids.push(node_id.to_string());
        }
        if self.state.focused_node_id.is_none() {
            self.state.focused_node_id = Some(node_id.to_string());
        }
        if let Some(lid) = rt.loop_id.clone() {
            if let Some(lp) = self.state.loops.get_mut(&lid) {
                if lp.phase != LoopPhase::Paused {
                    lp.phase = LoopPhase::Running;
                }
                lp.active_station_id = Some(node_id.to_string());
            }
        }
        Ok(())
    }

    fn render_effective_spec(&self, node_id: &str) -> GraphResult<String> {
        let n = self.node_plan(node_id)?;
        let cursor = self
            .state
            .nodes
            .get(node_id)
            .and_then(|r| r.loop_id.as_ref())
            .and_then(|lid| self.state.loops.get(lid))
            .map(|l| l.cursor.clone())
            .unwrap_or_default();
        let extras = HashMap::new();
        if let Some(t) = &n.spec_template {
            Ok(render_template(t, &cursor, &extras))
        } else {
            Ok(n.spec.clone().unwrap_or_default())
        }
    }

    pub fn record_file_touch(&mut self, node_id: &str, path: &str, op: &str) -> GraphResult<()> {
        let rt = self
            .state
            .nodes
            .get_mut(node_id)
            .ok_or_else(|| GraphError::NodeNotFound(node_id.into()))?;
        rt.files_touched_journal.push(FileTouch {
            path: path.into(),
            op: op.into(),
        });
        Ok(())
    }

    pub fn set_pending_summary(&mut self, node_id: &str, summary: String) -> GraphResult<()> {
        let rt = self
            .state
            .nodes
            .get_mut(node_id)
            .ok_or_else(|| GraphError::NodeNotFound(node_id.into()))?;
        rt.pending_summary = Some(summary);
        Ok(())
    }

    pub fn submit_for_approval(&mut self, node_id: &str) -> GraphResult<()> {
        let plan_human = self
            .node_plan(node_id)?
            .acceptance
            .human
            .as_ref()
            .map(|h| h.required)
            .unwrap_or(false);
        let rt = self
            .state
            .nodes
            .get_mut(node_id)
            .ok_or_else(|| GraphError::NodeNotFound(node_id.into()))?;
        if !matches!(rt.status, NodeStatus::Running | NodeStatus::Verifying) {
            return Err(GraphError::IllegalTransition(format!(
                "submit from {:?}",
                rt.status
            )));
        }
        // Plan human gate OR author intervened this iteration → must await approval.
        if plan_human || rt.human_intervened {
            rt.status = NodeStatus::AwaitingApproval;
        } else {
            rt.status = NodeStatus::Verifying;
        }
        Ok(())
    }

    /// Mark that the author drove substantive work on this node (Write/Edit in a user turn).
    pub fn mark_human_intervened(&mut self, node_id: &str) -> GraphResult<()> {
        let rt = self
            .state
            .nodes
            .get_mut(node_id)
            .ok_or_else(|| GraphError::NodeNotFound(node_id.into()))?;
        rt.human_intervened = true;
        Ok(())
    }

    pub fn mark_verifying(&mut self, node_id: &str) -> GraphResult<()> {
        let rt = self
            .state
            .nodes
            .get_mut(node_id)
            .ok_or_else(|| GraphError::NodeNotFound(node_id.into()))?;
        rt.status = NodeStatus::Verifying;
        Ok(())
    }

    pub fn approve(
        &mut self,
        work_root: &Path,
        node_id: &str,
    ) -> GraphResult<Option<LoopAdvanceEvent>> {
        let rt = self
            .state
            .nodes
            .get(node_id)
            .ok_or_else(|| GraphError::NodeNotFound(node_id.into()))?;
        if !matches!(
            rt.status,
            NodeStatus::AwaitingApproval | NodeStatus::Verifying | NodeStatus::Running
        ) {
            return Err(GraphError::IllegalTransition(format!(
                "approve from {:?}",
                rt.status
            )));
        }
        self.achieve_node(work_root, node_id)
    }

    pub fn reject(&mut self, node_id: &str, note: &str) -> GraphResult<()> {
        let plan_n = self.node_plan(node_id)?;
        let on_reject = plan_n
            .acceptance
            .human
            .as_ref()
            .map(|h| h.on_reject)
            .unwrap_or(OnReject::Continue);
        let max_iterations = plan_n.iterate.as_ref().map(|i| i.max_iterations);
        let status = self
            .state
            .nodes
            .get(node_id)
            .ok_or_else(|| GraphError::NodeNotFound(node_id.into()))?
            .status;
        if status != NodeStatus::AwaitingApproval {
            return Err(GraphError::IllegalTransition(format!(
                "reject from {:?}",
                status
            )));
        }
        let rt = self
            .state
            .nodes
            .get_mut(node_id)
            .ok_or_else(|| GraphError::NodeNotFound(node_id.into()))?;
        match on_reject {
            OnReject::Continue => {
                if let Some(max) = max_iterations {
                    if rt.iteration + 1 > max {
                        rt.feedback = Some(format!(
                            "{note}\n\n[max_iterations={max} reached — stay AwaitingApproval]"
                        ));
                        return Ok(());
                    }
                }
                rt.status = NodeStatus::Running;
                rt.feedback = Some(note.to_string());
                rt.iteration += 1;
            }
            OnReject::Fail => {
                rt.status = NodeStatus::Failed;
                rt.feedback = Some(note.to_string());
                self.state.running_node_ids.retain(|id| id != node_id);
            }
        }
        Ok(())
    }

    fn achieve_node(
        &mut self,
        work_root: &Path,
        node_id: &str,
    ) -> GraphResult<Option<LoopAdvanceEvent>> {
        let summary = self
            .state
            .nodes
            .get(node_id)
            .and_then(|r| r.pending_summary.clone())
            .filter(|s| !s.trim().is_empty())
            .ok_or_else(|| {
                GraphError::IllegalTransition(
                    "Achieved requires non-empty pending_summary (non-CoT wrap-up)".into(),
                )
            })?;
        let files = self
            .state
            .nodes
            .get(node_id)
            .map(|r| r.files_touched_journal.clone())
            .unwrap_or_default();
        let artifacts = self.writable_paths(node_id)?;
        let handoff = NodeHandoff {
            node_id: node_id.to_string(),
            summary,
            files_touched: files,
            artifacts,
            achieved_at: Some(Utc::now().to_rfc3339()),
        };
        crate::persist::save_handoff(work_root, &handoff)?;
        // Archive snapshot copy so history survives loop-advance overwrites.
        let snapshot_key = self
            .state
            .nodes
            .get(node_id)
            .and_then(|r| r.loop_id.as_ref())
            .and_then(|lid| self.state.loops.get(lid))
            .map(|l| snapshot_key_from_cursor(&l.cursor));
        if let Some(ref key) = snapshot_key {
            if let Err(e) = crate::persist::save_handoff_snapshot(work_root, &handoff, key) {
                tracing::warn!(error = %e, node_id, snapshot_key = key, "save_handoff_snapshot failed");
            }
        }
        crate::persist::append_jsonl(
            work_root,
            &json!({
                "event": "achieved",
                "node_id": node_id,
                "at": handoff.achieved_at,
            }),
        )?;

        let loop_id = self
            .state
            .nodes
            .get(node_id)
            .and_then(|r| r.loop_id.clone());
        {
            let rt = self
                .state
                .nodes
                .get_mut(node_id)
                .ok_or_else(|| GraphError::NodeNotFound(node_id.into()))?;
            rt.status = NodeStatus::Achieved;
            rt.handoff = Some(handoff);
            rt.human_intervened = false;
            rt.pending_summary = None;
            rt.files_touched_journal.clear();
        }
        self.state.running_node_ids.retain(|id| id != node_id);
        self.recompute_ready();

        let mut advance = None;
        if let Some(lid) = loop_id {
            advance = self.maybe_advance_loop(work_root, &lid, node_id)?;
        }
        Ok(advance)
    }

    fn maybe_advance_loop(
        &mut self,
        work_root: &Path,
        loop_id: &str,
        achieved_station: &str,
    ) -> GraphResult<Option<LoopAdvanceEvent>> {
        let lp_plan = self
            .plan
            .loops
            .iter()
            .find(|l| l.id == loop_id)
            .ok_or_else(|| GraphError::LoopNotFound(loop_id.into()))?
            .clone();
        if achieved_station != lp_plan.advance_after {
            return Ok(None);
        }
        let rt = self
            .state
            .loops
            .get(loop_id)
            .ok_or_else(|| GraphError::LoopNotFound(loop_id.into()))?;
        if rt.phase == LoopPhase::Paused {
            return Ok(None);
        }

        // Evaluate termination condition.
        if evaluate_until(&lp_plan.until, &rt.cursor, &self.state.settings.settings) {
            if let Some(l) = self.state.loops.get_mut(loop_id) {
                l.phase = LoopPhase::Completed;
                l.active_station_id = None;
            }
            crate::persist::append_jsonl(
                work_root,
                &json!({"event":"loop_completed","loop_id": loop_id}),
            )?;
            return Ok(None);
        }

        // Advance: apply AdvanceRule to cursor.
        let now = Utc::now().to_rfc3339();
        let advance_rule = lp_plan.advance.clone();
        let snapshot_key;
        {
            let l = self
                .state
                .loops
                .get_mut(loop_id)
                .ok_or_else(|| GraphError::LoopNotFound(loop_id.into()))?;
            l.phase = LoopPhase::Advancing;

            // Primary increment.
            let cur = l
                .cursor
                .counters
                .get(&advance_rule.increment)
                .copied()
                .unwrap_or(0);
            l.cursor
                .counters
                .insert(advance_rule.increment.clone(), cur + advance_rule.step);

            // Side effects.
            for op in &advance_rule.side_effects {
                apply_counter_op(&mut l.cursor.counters, op);
            }

            snapshot_key = snapshot_key_from_cursor(&l.cursor);
            l.last_advance_at = Some(now.clone());
            l.active_station_id = Some(lp_plan.entry.clone());
        }
        let counters_snapshot = self
            .state
            .loops
            .get(loop_id)
            .map(|l| l.cursor.counters.clone())
            .unwrap_or_default();

        // Snapshot the advance_after handoff BEFORE clearing runtime state,
        // so the next iteration's entry node receives continuity context.
        let prev_handoff = self
            .state
            .nodes
            .get(&lp_plan.advance_after)
            .and_then(|r| r.handoff.clone());

        for sid in &lp_plan.on_advance.reopen {
            if let Some(n) = self.state.nodes.get_mut(sid) {
                n.status = NodeStatus::Waiting;
                clear_iteration_runtime(n);
                if lp_plan.on_advance.reinject_objectives {
                    n.effective_spec = None;
                }
            }
            self.state.running_node_ids.retain(|id| id != sid);
        }
        // preserve_canon_files (default true): advance never deletes world_state_board paths.
        if !lp_plan.on_advance.preserve_canon_files {
            tracing::warn!(
                loop_id,
                "on_advance.preserve_canon_files=false is ignored; Loop never deletes world_state_board files"
            );
        }
        self.recompute_ready();
        // Only force entry Ready if its deps (outside the reopen set) are still Achieved.
        let deps_ok = self
            .node_plan(&lp_plan.entry)
            .map(|pn| {
                pn.deps.iter().all(|d| {
                    self.state
                        .nodes
                        .get(d)
                        .map(|r| r.status == NodeStatus::Achieved)
                        .unwrap_or(false)
                })
            })
            .unwrap_or(true);
        if deps_ok {
            if let Some(n) = self.state.nodes.get_mut(&lp_plan.entry) {
                n.status = NodeStatus::Ready;
            }
        }
        {
            let l = self
                .state
                .loops
                .get_mut(loop_id)
                .ok_or_else(|| GraphError::LoopNotFound(loop_id.into()))?;
            l.prev_iteration_handoff = prev_handoff;
            l.phase = LoopPhase::Running;
        }
        // Cursor advanced — drop focus so next turn does not keep a reset station's NodeObjective.
        if self
            .state
            .focused_node_id
            .as_ref()
            .is_some_and(|fid| lp_plan.on_advance.reopen.contains(fid))
        {
            self.state.focused_node_id = None;
        }
        crate::persist::append_jsonl(
            work_root,
            &json!({
                "event": "loop_advanced",
                "loop_id": loop_id,
                "snapshot_key": snapshot_key,
                "counters": counters_snapshot,
                "at": now,
            }),
        )?;
        Ok(Some(LoopAdvanceEvent {
            loop_id: loop_id.to_string(),
            snapshot_key,
            counters: counters_snapshot,
            reset_node_ids: lp_plan.on_advance.reopen.clone(),
        }))
    }

    pub fn pause_loop(&mut self, loop_id: &str, reason: &str) -> GraphResult<()> {
        let l = self
            .state
            .loops
            .get_mut(loop_id)
            .ok_or_else(|| GraphError::LoopNotFound(loop_id.into()))?;
        l.phase = LoopPhase::Paused;
        l.paused_reason = Some(reason.into());
        Ok(())
    }

    pub fn resume_loop(&mut self, loop_id: &str) -> GraphResult<()> {
        let l = self
            .state
            .loops
            .get_mut(loop_id)
            .ok_or_else(|| GraphError::LoopNotFound(loop_id.into()))?;
        if l.phase == LoopPhase::Completed {
            return Err(GraphError::IllegalTransition("loop completed".into()));
        }
        l.phase = LoopPhase::Running;
        l.paused_reason = None;
        Ok(())
    }

    pub fn reopen(&mut self, node_id: &str, cascade_downstream: bool) -> GraphResult<Vec<String>> {
        let max = self.node_plan(node_id)?.rollback.max_regates;
        let rt = self
            .state
            .nodes
            .get_mut(node_id)
            .ok_or_else(|| GraphError::NodeNotFound(node_id.into()))?;
        if rt.regate_count >= max {
            return Err(GraphError::IllegalTransition(format!(
                "max_regates={max} exceeded for {node_id}"
            )));
        }
        rt.regate_count += 1;
        rt.status = NodeStatus::Ready;
        clear_iteration_runtime(rt);
        self.state.running_node_ids.retain(|id| id != node_id);

        let mut demoted = vec![node_id.to_string()];
        if cascade_downstream {
            let downstream = self.downstream_closure(node_id);
            for d in &downstream {
                if let Some(n) = self.state.nodes.get_mut(d) {
                    if matches!(
                        n.status,
                        NodeStatus::Achieved
                            | NodeStatus::Running
                            | NodeStatus::Verifying
                            | NodeStatus::AwaitingApproval
                            | NodeStatus::Ready
                            | NodeStatus::Failed
                    ) {
                        n.status = NodeStatus::Waiting;
                        clear_iteration_runtime(n);
                        demoted.push(d.clone());
                    }
                }
                self.state.running_node_ids.retain(|id| id != d);
            }
        }
        self.recompute_ready();
        Ok(demoted)
    }

    fn downstream_closure(&self, node_id: &str) -> Vec<String> {
        let mut children: HashMap<&str, Vec<&str>> = HashMap::new();
        for n in &self.plan.nodes {
            for d in &n.deps {
                children.entry(d.as_str()).or_default().push(n.id.as_str());
            }
        }
        let mut out = Vec::new();
        let mut stack = vec![node_id];
        let mut seen = HashSet::new();
        while let Some(u) = stack.pop() {
            if let Some(chs) = children.get(u) {
                for c in chs {
                    if seen.insert(*c) {
                        out.push((*c).to_string());
                        stack.push(c);
                    }
                }
            }
        }
        out
    }

    pub fn demote_on_edit(&mut self, path: &str) -> Vec<String> {
        let board: HashSet<String> = self
            .plan
            .loops
            .iter()
            .flat_map(|lp| lp.world_state_board.clone())
            .collect();
        let path_n = path.replace('\\', "/");
        if board.iter().any(|b| {
            let b = b.replace('\\', "/");
            path_n == b || path_n.starts_with(&b) || (b.ends_with('/') && path_n.starts_with(&b))
        }) {
            return Vec::new();
        }
        let mut demoted = Vec::new();
        let mut targets = Vec::new();
        for n in &self.plan.nodes {
            if let Ok(paths) = self.writable_paths(&n.id) {
                if paths.iter().any(|p| {
                    let p = p.replace('\\', "/");
                    path_n == p || path_n.starts_with(&format!("{p}/"))
                }) {
                    let st = self
                        .state
                        .nodes
                        .get(&n.id)
                        .map(|r| r.status)
                        .unwrap_or(NodeStatus::Waiting);
                    // Do not demote the node that is currently writing.
                    let writing = self.state.focused_node_id.as_deref() == Some(n.id.as_str())
                        || self.state.running_node_ids.iter().any(|id| id == &n.id);
                    if st == NodeStatus::Achieved && !writing {
                        targets.push(n.id.clone());
                    }
                }
            }
        }
        for id in targets {
            if let Ok(d) = self.reopen(&id, true) {
                demoted.extend(d);
            }
        }
        demoted
    }

    fn upstream_handoffs(&self, node_id: &str) -> GraphResult<Vec<NodeHandoff>> {
        let n = self.node_plan(node_id)?;
        let mut out = Vec::new();
        for d in &n.deps {
            if let Some(h) = self.state.nodes.get(d).and_then(|r| r.handoff.clone()) {
                out.push(h);
            }
        }
        Ok(out)
    }

    pub fn node_objective_block(&self, node_id: &str) -> GraphResult<String> {
        let n = self.node_plan(node_id)?;
        let rt = self.state.nodes.get(node_id);
        let spec = rt
            .and_then(|r| r.effective_spec.clone())
            .unwrap_or_else(|| {
                self.render_effective_spec(node_id)
                    .unwrap_or_else(|_| n.spec.clone().unwrap_or_default())
            });
        let mut parts = vec![
            format!("## NodeObjective [{id}]", id = n.id),
            format!("Title: {}", n.title),
        ];

        // Stable upstream handoffs first: identical across loop iterations → KV-cache reuse.
        let handoffs = self.upstream_handoffs(node_id)?;
        if !handoffs.is_empty() {
            parts.push("## Upstream handoffs".into());
            for h in handoffs {
                parts.push(format_handoff(&h));
            }
        }

        // Cursor-rendered spec: only the counter digit changes (ch1→ch2), minimal KV break.
        parts.push(spec);

        // Previous-iteration handoff: fully new each round, placed last so everything
        // above benefits from KV-cache reuse.
        if let Some(prev) = self.prev_iteration_handoff(rt, node_id) {
            parts.push("## Previous iteration summary".into());
            parts.push(format_handoff(&prev));
        }

        if let Some(fb) = rt.and_then(|r| r.feedback.clone()) {
            parts.push(format!("## Author feedback (reject continue)\n{fb}"));
        }
        Ok(parts.join("\n\n"))
    }

    /// If `node_id` is a loop entry station and the loop has advanced at least once,
    /// return the previous iteration's advance_after handoff for continuity context.
    fn prev_iteration_handoff(
        &self,
        rt: Option<&GraphNodeRuntime>,
        node_id: &str,
    ) -> Option<NodeHandoff> {
        let lid = rt?.loop_id.as_ref()?;
        let lp = self.plan.loops.iter().find(|l| &l.id == lid)?;
        if lp.entry != node_id {
            return None;
        }
        self.state.loops.get(lid)?.prev_iteration_handoff.clone()
    }
}

/// Clear per-iteration fields when a station is reset (loop advance / cursor / reopen).
/// Always clears `human_intervened` so iteration N intervention does not leak to N+1.
fn clear_iteration_runtime(rt: &mut GraphNodeRuntime) {
    rt.feedback = None;
    rt.pending_summary = None;
    rt.files_touched_journal.clear();
    rt.handoff = None;
    rt.human_intervened = false;
}

/// Format a single handoff block for prompt injection.
fn format_handoff(h: &NodeHandoff) -> String {
    let mut lines = vec![
        format!("### From node {}", h.node_id),
        "### What was done".into(),
        h.summary.clone(),
        "### Files written or modified".into(),
    ];
    for f in &h.files_touched {
        lines.push(format!("- {} ({})", f.path, f.op));
    }
    lines.push("### Declared artifacts".into());
    for a in &h.artifacts {
        lines.push(format!("- {a}"));
    }
    lines.join("\n")
}

#[derive(Debug, Clone)]
pub struct LoopAdvanceEvent {
    pub loop_id: String,
    pub snapshot_key: String,
    pub counters: HashMap<String, i64>,
    pub reset_node_ids: Vec<String>,
}

// ── Helper: evaluate Until condition ────────────────────────────────

fn resolve_value(
    value: &Option<i64>,
    value_from: &Option<String>,
    settings: &HashMap<String, serde_json::Value>,
) -> Option<i64> {
    if let Some(v) = *value {
        return Some(v);
    }
    if let Some(ref path) = *value_from {
        // Try direct key lookup in settings
        if let Some(val) = settings.get(path.as_str()) {
            return val.as_i64();
        }
        // Dotted path: "work_meta.settings.targetChapters" → use last segment
        if let Some(last) = path.split('.').next_back() {
            if let Some(val) = settings.get(last) {
                return val.as_i64();
            }
        }
    }
    None
}

fn evaluate_until(
    until: &Until,
    cursor: &crate::types::Cursor,
    settings: &HashMap<String, serde_json::Value>,
) -> bool {
    match until {
        Until::CounterGt {
            counter,
            value,
            value_from,
        } => {
            let threshold = match resolve_value(value, value_from, settings) {
                Some(v) => v,
                None => return false,
            };
            let cur = cursor.counters.get(counter).copied().unwrap_or(0);
            cur > threshold
        }
        Until::CounterGe {
            counter,
            value,
            value_from,
        } => {
            let threshold = match resolve_value(value, value_from, settings) {
                Some(v) => v,
                None => return false,
            };
            let cur = cursor.counters.get(counter).copied().unwrap_or(0);
            cur >= threshold
        }
        Until::CounterLt {
            counter,
            value,
            value_from,
        } => {
            let threshold = match resolve_value(value, value_from, settings) {
                Some(v) => v,
                None => return false,
            };
            let cur = cursor.counters.get(counter).copied().unwrap_or(0);
            cur < threshold
        }
        Until::CounterEq {
            counter,
            value,
            value_from,
        } => {
            let threshold = match resolve_value(value, value_from, settings) {
                Some(v) => v,
                None => return false,
            };
            let cur = cursor.counters.get(counter).copied().unwrap_or(0);
            cur == threshold
        }
        Until::Manual => false,
    }
}

fn apply_counter_op(counters: &mut HashMap<String, i64>, op: &CounterOp) {
    match op {
        CounterOp::Increment { counter, by } => {
            let cur = counters.get(counter).copied().unwrap_or(0);
            counters.insert(counter.clone(), cur + by);
        }
        CounterOp::Decrement { counter, by } => {
            let cur = counters.get(counter).copied().unwrap_or(0);
            counters.insert(counter.clone(), cur.saturating_sub(*by));
        }
        CounterOp::Set { counter, value } => {
            counters.insert(counter.clone(), *value);
        }
        CounterOp::Reset { counter } => {
            counters.insert(counter.clone(), 0);
        }
    }
}

/// Build a human-readable snapshot key from cursor counters.
/// Sorts keys alphabetically for deterministic output.
fn snapshot_key_from_cursor(cursor: &crate::types::Cursor) -> String {
    let mut keys: Vec<&String> = cursor.counters.keys().collect();
    keys.sort();
    if let Some(key) = keys.first() {
        let val = cursor.counters.get(*key).copied().unwrap_or(0);
        format!("{key}={val}")
    } else {
        String::new()
    }
}

#[cfg(test)]
mod evaluate_until_tests {
    //! Boundaries for evaluate_until / resolve_value:
    //! - each Until variant (Gt/Ge/Lt/Eq/Manual)
    //! - missing counter → treat as 0
    //! - resolve: explicit value; settings direct key; dotted path last segment; unresolved → false

    use super::{evaluate_until, resolve_value};
    use crate::types::{Cursor, Until};
    use std::collections::HashMap;

    fn cursor(n: i64) -> Cursor {
        let mut counters = HashMap::new();
        counters.insert("n".into(), n);
        Cursor {
            counters,
            tags: HashMap::new(),
        }
    }

    fn settings(pairs: &[(&str, i64)]) -> HashMap<String, serde_json::Value> {
        pairs
            .iter()
            .map(|(k, v)| ((*k).into(), serde_json::json!(*v)))
            .collect()
    }

    #[test]
    fn resolve_value_edges() {
        let s = settings(&[("target", 10), ("targetChapters", 200)]);
        assert_eq!(resolve_value(&Some(3), &None, &s), Some(3));
        assert_eq!(resolve_value(&None, &Some("target".into()), &s), Some(10));
        assert_eq!(
            resolve_value(&None, &Some("work_meta.settings.targetChapters".into()), &s),
            Some(200)
        );
        assert_eq!(resolve_value(&None, &Some("missing".into()), &s), None);
        assert_eq!(resolve_value(&None, &None, &s), None);
    }

    #[test]
    fn until_comparisons_and_manual() {
        let s = settings(&[]);
        assert!(!evaluate_until(
            &Until::CounterGt {
                counter: "n".into(),
                value: Some(5),
                value_from: None
            },
            &cursor(5),
            &s
        ));
        assert!(evaluate_until(
            &Until::CounterGt {
                counter: "n".into(),
                value: Some(5),
                value_from: None
            },
            &cursor(6),
            &s
        ));
        assert!(evaluate_until(
            &Until::CounterGe {
                counter: "n".into(),
                value: Some(5),
                value_from: None
            },
            &cursor(5),
            &s
        ));
        assert!(evaluate_until(
            &Until::CounterLt {
                counter: "n".into(),
                value: Some(5),
                value_from: None
            },
            &cursor(4),
            &s
        ));
        assert!(evaluate_until(
            &Until::CounterEq {
                counter: "n".into(),
                value: Some(5),
                value_from: None
            },
            &cursor(5),
            &s
        ));
        assert!(!evaluate_until(&Until::Manual, &cursor(99), &s));
        // missing counter → 0; unresolved threshold → false (all counter variants)
        for until in [
            Until::CounterGt {
                counter: "ghost".into(),
                value: None,
                value_from: Some("nope".into()),
            },
            Until::CounterGe {
                counter: "ghost".into(),
                value: None,
                value_from: Some("nope".into()),
            },
            Until::CounterLt {
                counter: "ghost".into(),
                value: None,
                value_from: Some("nope".into()),
            },
            Until::CounterEq {
                counter: "ghost".into(),
                value: None,
                value_from: Some("nope".into()),
            },
        ] {
            assert!(!evaluate_until(&until, &cursor(9), &s));
        }
        // value_from from settings
        let s = settings(&[("cap", 3)]);
        assert!(evaluate_until(
            &Until::CounterGe {
                counter: "n".into(),
                value: None,
                value_from: Some("cap".into())
            },
            &cursor(3),
            &s
        ));
    }
}
