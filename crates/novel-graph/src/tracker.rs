//! GraphTracker: status transitions, ready recompute, parallel start, handoff, loop advance.

use crate::error::{GraphError, GraphResult};
use crate::template::render_template;
use crate::types::{
    ArtifactRole, FileTouch, GraphLoopRuntime, GraphNodeRuntime, GraphSettings, GraphState,
    LoopPhase, NodeHandoff, NodeStatus, OnReject, PlanGraph, PlanNode,
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
                target_chapters: plan.target_chapters,
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
                target_chapters: plan.target_chapters,
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

    pub fn start_node(&mut self, node_id: &str, session_id: Option<String>) -> GraphResult<()> {
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
        let rt = self.state.nodes.get_mut(node_id).expect("checked");
        rt.status = NodeStatus::Running;
        rt.session_id = session_id;
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

    pub fn render_effective_spec(&self, node_id: &str) -> GraphResult<String> {
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

    pub fn achieve_node(
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
        // Archive per-chapter copy so history survives loop-advance overwrites.
        let current_chapter = self
            .state
            .nodes
            .get(node_id)
            .and_then(|r| r.loop_id.as_ref())
            .and_then(|lid| self.state.loops.get(lid))
            .map(|l| l.cursor.chapter);
        if let Some(ch) = current_chapter {
            if let Err(e) = crate::persist::save_handoff_chapter(work_root, &handoff, ch) {
                tracing::warn!(error = %e, node_id, chapter = ch, "save_handoff_chapter failed");
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
            let rt = self.state.nodes.get_mut(node_id).expect("exists");
            rt.status = NodeStatus::Achieved;
            rt.handoff = Some(handoff);
            rt.session_id = None;
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
        let target = self
            .state
            .settings
            .target_chapters
            .or(self.plan.target_chapters)
            .or(lp_plan.until.value)
            .unwrap_or(u32::MAX);
        let next_chapter = rt.cursor.chapter + 1;
        if next_chapter > target {
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
        // advance
        let now = Utc::now().to_rfc3339();
        {
            let l = self.state.loops.get_mut(loop_id).expect("exists");
            l.phase = LoopPhase::Advancing;
            l.cursor.chapter = next_chapter;
            l.cursor.round += 1;
            l.last_advance_at = Some(now.clone());
            l.active_station_id = Some(lp_plan.entry.clone());
        }
        for sid in &lp_plan.on_advance.reopen {
            if let Some(n) = self.state.nodes.get_mut(sid) {
                n.status = NodeStatus::Waiting;
                clear_iteration_runtime(n, lp_plan.on_advance.clear_node_sessions);
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
                "on_advance.preserve_canon_files=false is ignored; Book Loop never deletes world_state_board files"
            );
        }
        self.recompute_ready();
        // Only force entry Ready if its deps (that are outside the reopen set) are still Achieved.
        // If a skeleton ancestor was cascade-demoted, entry stays Waiting to avoid bypassing deps.
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
            let l = self.state.loops.get_mut(loop_id).expect("exists");
            l.phase = LoopPhase::Running;
        }
        // Chapter rolled — drop focus so next turn does not keep a reset station's NodeObjective.
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
                "chapter": next_chapter,
                "at": now,
            }),
        )?;
        Ok(Some(LoopAdvanceEvent {
            loop_id: loop_id.to_string(),
            chapter: next_chapter,
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

    pub fn set_loop_target(&mut self, loop_id: &str, target: u32) -> GraphResult<()> {
        if !self.plan.loops.iter().any(|l| l.id == loop_id)
            && !self.state.loops.contains_key(loop_id)
        {
            return Err(GraphError::LoopNotFound(loop_id.into()));
        }
        // Work-level target (all loops share settings.target_chapters).
        self.state.settings.target_chapters = Some(target);
        Ok(())
    }

    pub fn set_loop_cursor(&mut self, loop_id: &str, chapter: u32) -> GraphResult<()> {
        // Demote all active loop stations so they re-render with the new cursor value.
        let stations: Vec<String> = self
            .plan
            .loops
            .iter()
            .find(|l| l.id == loop_id)
            .map(|lp| lp.on_advance.reopen.clone())
            .unwrap_or_default();
        for sid in &stations {
            if let Some(n) = self.state.nodes.get_mut(sid) {
                if matches!(
                    n.status,
                    NodeStatus::Running
                        | NodeStatus::Ready
                        | NodeStatus::Verifying
                        | NodeStatus::AwaitingApproval
                ) {
                    n.status = NodeStatus::Waiting;
                    clear_iteration_runtime(n, true);
                    n.effective_spec = None;
                }
            }
            self.state.running_node_ids.retain(|id| id != sid);
        }
        self.recompute_ready();
        // After demoting, force the entry station Ready so the loop can start.
        let entry = self
            .plan
            .loops
            .iter()
            .find(|l| l.id == loop_id)
            .map(|lp| lp.entry.clone());
        if let Some(ref eid) = entry {
            // Check deps before taking mutable borrow on nodes.
            let deps_ok = self
                .node_plan(eid)
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
                if let Some(n) = self.state.nodes.get_mut(eid) {
                    n.status = NodeStatus::Ready;
                }
            }
        }
        {
            let l = self
                .state
                .loops
                .get_mut(loop_id)
                .ok_or_else(|| GraphError::LoopNotFound(loop_id.into()))?;
            l.cursor.chapter = chapter;
            l.cursor.round = chapter;
        }
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
        clear_iteration_runtime(rt, true);
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
                        clear_iteration_runtime(n, true);
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

    pub fn upstream_handoffs(&self, node_id: &str) -> GraphResult<Vec<NodeHandoff>> {
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
        let spec = self
            .state
            .nodes
            .get(node_id)
            .and_then(|r| r.effective_spec.clone())
            .unwrap_or_else(|| {
                self.render_effective_spec(node_id)
                    .unwrap_or_else(|_| n.spec.clone().unwrap_or_default())
            });
        let mut parts = vec![
            format!("## NodeObjective [{id}]", id = n.id),
            format!("Title: {}", n.title),
            spec,
        ];
        if let Some(fb) = self
            .state
            .nodes
            .get(node_id)
            .and_then(|r| r.feedback.clone())
        {
            parts.push(format!("## Author feedback (reject continue)\n{fb}"));
        }
        let handoffs = self.upstream_handoffs(node_id)?;
        if !handoffs.is_empty() {
            parts.push("## Upstream handoffs".into());
            for h in handoffs {
                parts.push(format!("### From node {}", h.node_id));
                parts.push("### What was done".into());
                parts.push(h.summary);
                parts.push("### Files written or modified".into());
                for f in h.files_touched {
                    parts.push(format!("- {} ({})", f.path, f.op));
                }
                parts.push("### Declared artifacts".into());
                for a in h.artifacts {
                    parts.push(format!("- {a}"));
                }
            }
        }
        Ok(parts.join("\n\n"))
    }
}

/// Clear per-iteration fields when a station is reset (loop advance / cursor / reopen).
/// Always clears `human_intervened` so chapter N intervention does not leak to N+1.
fn clear_iteration_runtime(rt: &mut GraphNodeRuntime, clear_session: bool) {
    if clear_session {
        rt.session_id = None;
    }
    rt.feedback = None;
    rt.pending_summary = None;
    rt.files_touched_journal.clear();
    rt.handoff = None;
    rt.human_intervened = false;
}

#[derive(Debug, Clone)]
pub struct LoopAdvanceEvent {
    pub loop_id: String,
    pub chapter: u32,
    pub reset_node_ids: Vec<String>,
}
