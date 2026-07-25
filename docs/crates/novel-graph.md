# novel-graph — Graph-Primary 编排

> 所属项目: [Novel Agent](../../README.md)

---

## 1. 业务逻辑

仅依赖 `novel-config`（禁止依赖 core/tools/server/state/knowledge）。`novel-graph` 持有 plan schema、GraphTracker、Book Loop 运行时、handoff、门禁与 checkpoint，不调用 AgentEngine 或 Tool trait。

### 1.1 Plan Schema

作品级 SSOT 为 `knowledge/meta/plan-graph.json`（[默认骨架](../../templates/knowledge/meta/plan-graph.json)）。核心类型：

| 类型 | 字段要点 |
|------|---------|
| `PlanGraph` | `version`、`nodes`、`loops`、`enforce_gates`、`max_parallel_nodes`、`target_chapters`、`auto_start_ready` |
| `PlanNode` | `id`、`title`、`spec` / `spec_template`、`deps`、`kind`、`artifacts`、`acceptance`、`rollback`、`iterate` |
| `BookLoop` | `id`、`stations`、`entry`、`advance_after`、`cursor`、`until`、`on_advance`、`world_state_board` |
| `Acceptance` | `machine`（None / Verifier / Auditor）+ 可选 `HumanGate { required, on_reject, prompt, review }` |
| `Rollback` | `allowed_targets`、`explicit_ids`、`max_regates`（默认 3） |

**校验（`parse_and_validate`）：** DAG 无环（循环须用 reopen policy，非 deps 边）、deps 节点存在、loop stations 引用存在、artifact path 不逃逸工作根、`human.required` 时建议含 `primary_deliverable`。

**模板渲染：** `spec_template` / `path_template` 使用 `{{cursor.chapter}}`、`{{cursor.chapter | pad3}}` 等占位符；start_node 时由 harness 渲染为 `effective_spec`。

### 1.2 GraphTracker

状态机：

```
Waiting → Ready → Running ⇄ (作者对话/改产物)
                      ↓
                 Verifying（机器/审计 Skill）
                      ↓
              AwaitingApproval ⇄ Running   ← reject+feedback（continue）
                      ↓ approve
                   Achieved → 解锁 deps 下游
```

**核心方法：**

| 方法 | 说明 |
|------|------|
| `new` / `load` / `save` | plan → state 初始化/持久化；`load` 合并新增节点与 loop 运行时 |
| `recompute_ready` | deps 全 Achieved → Ready（扇出：多后继可同时 Ready） |
| `start_node` | Ready → Running；写冲突检测（`write_overlap`）、并行上限门控（`max_parallel_nodes`）、渲染 `effective_spec`、auto-focus |
| `submit_for_approval` | plan `human.required` **或** 运行时 `human_intervened` → AwaitingApproval；否则 → Verifying |
| `approve` | AwaitingApproval / Verifying / Running → Achieved → 固化 NodeHandoff → `recompute_ready` |
| `reject` | AwaitingApproval → Running（continue）或 Failed（fail）；达 `iterate.max_iterations` 保持 AwaitingApproval |
| `reopen` | 目标 + 可选下游闭包 → Waiting/Ready（`regate_count++`；超 `max_regates` 拒绝） |
| `set_focus` | 设置 `focused_node_id`（作者聊天焦点，至多一个） |
| `writable_paths` | 渲染当前游标的 artifact path 集（含 `path_template` → 实际路径） |

**扇出并行：** 多 `running_node_ids` 可同时执行；`focused_node_id` 仅作者焦点。写路径相交 → `parallel_blocked: write_overlap`，拒绝 start 并记 graph.jsonl。默认 `max_parallel_nodes = 4`。

**set_loop_cursor：** 跳章前 demote 本轮所有活跃 station（Running/Ready/Verifying/AwaitingApproval → Waiting），清空 session/effective_spec，然后强制 entry Ready（若 skeleton deps 仍满足）。

### 1.3 Book Loop

少节点游标循环，禁止 deps 成环。

| 概念 | 说明 |
|------|------|
| `cursor` | `{ chapter, volume, fine_outline_through, round }` — 跨轮持久 |
| `advance_after` | 当该 station Achieved 时触发 advance |
| `on_advance.reopen` | 需重置的 station id 列表（clear session/effective_spec/handoff/human_intervened） |
| `until` | `chapter_gt` + `value`（或 `value_from: work_meta.target_chapters`）→ 达上限 → Completed |
| `world_state_board` | 正典板文件集：写这些路径不触发 demote_on_edit；loop advance 不清除 |

**advance 流程：** `sync-canon` Achieved → chapter+1 → reopen stations → `recompute_ready` → force entry Ready（验 deps 满足）→ emit `graph-loop-changed` + `node-session-reset`。

**章间隔离：** advance/reopen 清空 `human_intervened`、`pending_summary`、`files_touched_journal`、`handoff`。第 N 章的干预不影响 N+1。

### 1.4 门禁

| 门禁 | 条件 | 文件 |
|------|------|------|
| `check_write_allowed` | `enforce_gates` 时，path 须属某 Running 节点的 `writable_paths`，且其 deps 全 Achieved | [gate.rs](../../crates/novel-graph/src/gate.rs) |
| `graph_gate_write` | Write/Edit 前调，`enforce_gates: false` 或无 plan → 直通 | [graph_hook.rs](../../crates/novel-tools/src/graph_hook.rs) |
| `demote_on_edit` | Write/Edit 命中已 Achieved 节点的 primary_deliverable（非 `world_state_board`）→ reopen 该节点 + 下游闭包 → 写 `graph.jsonl` | [tracker.rs](../../crates/novel-graph/src/tracker.rs) |

门禁由 `graph_hook` 接入 tool 成功路径；demote 后通过 `on_graph_state_changed` 回调通知 UI。

### 1.5 节点交接

Achieved 出门时固化 `NodeHandoff`：

| 字段 | 来源 | 说明 |
|------|------|------|
| `summary` | `pending_summary`（非 CoT 收尾正文） | 缺则拒绝 Achieved |
| `files_touched` | Write/Edit journal | 引擎侧工具成功记录 |
| `artifacts` | plan `writable_paths` | 产物路径清单 |

**落盘：** `knowledge/meta/handoffs/{node_id}.json`（最新）+ `knowledge/meta/handoffs/{node_id}-ch-{NNN}.json`（按章归档，供 LoopHistory 读取）。

下游启动时注入 `node_objective_block`：
```
## NodeObjective [id]
Title: ...
spec / effective_spec

## Author feedback (reject continue) — if any

## Upstream handoffs — 每个已完成 deps 的 summary + files_touched + artifacts
```

### 1.6 初始化与模板

| 入口 | 行为 |
|------|------|
| `ensure_graph_initialized` | plan 不存在 → 写默认骨架 + state；plan 存在但损坏 → **返回 Err**（不覆盖） |
| `write_default_plan_file` | 仅缺失时写 plan + state |
| `graph_preview_template`（IPC） | 返回 bundled 默认 plan JSON，**不写盘** |
| `graph_apply_template`（IPC） | 写正式 plan + state（已有 plan 时需 `force`）→ emit `graph-plan-committed` |
| `open_work` | 仅在 `plan_exists` 时才 `ensure_graph_initialized`——不静默落图 |
| `init_novel_project` / `create_work` | 不 auto-seed plan |

默认骨架两处同文：`templates/knowledge/meta/plan-graph.template.json`（scaffold 参考）与 `crates/novel-graph/templates/plan-graph.json`（`include_str!`）；由 `init` 单测防漂移。

### 1.7 出门闸

两个条件任一 → `submit_for_approval` 置 `AwaitingApproval`：

| 条件 | 来源 |
|------|------|
| plan `human.required: true` | `plan-graph.json` 节点定义 |
| 运行时 `human_intervened` | 作者在 focus 节点上成功 Write/Edit → `mark_human_intervened` |

**`human_intervened` 生命周期：** 作者 turn 开始 → 若 focused_node 存在则武装 → Write/Edit 成功时置位 → submit 时 check → Achieved/advance 时清零（章间隔离）。Unattended（无作者 turn）不会武装，写章可自动 Achieved。仅阻塞目标节点出口，不冻结整张图。

### 1.8 IPC 与工具

**Tauri IPC commands（10 个）：**

| Command | 说明 |
|--------|------|
| `graph_get_state` | 全量 `GraphStateSnapshot`（含 `hasPlan` 标志） |
| `graph_get_node` | 单节点 plan + runtime + objective |
| `graph_get_loop` | 单 loop 详情 |
| `graph_activate_node` | 设置 `focused_node_id` |
| `graph_start_node` | Ready → Running |
| `graph_approve` / `graph_reject` / `graph_reopen` | 人审操作 |
| `graph_loop_pause` / `graph_loop_resume` | 暂停/继续 auto advance |
| `graph_loop_set_target` / `graph_loop_set_cursor` | 改目标章数 / 跳游标 |
| `graph_loop_list_history` | 按章 handoff 历史（读归档 `{node_id}-ch-{NNN}.json` + chapters/ 目录） |
| `graph_preview_template` / `graph_apply_template` | 模板预览 + 应用 |

**IPC Events（7 个）：**

| Event | 触发时机 |
|-------|---------|
| `graph-state-changed` | 节点状态 / focus / running 变更 |
| `graph-loop-changed` | cursor/phase 变更 |
| `graph-hitl` | HITL 角标变化 |
| `graph-approval-required` | AwaitingApproval 出现 |
| `graph-plan-committed` | 模板应用 / 正式图落盘 |
| `node-session-reset` | loop advance 清 session |

**Graph 工具（注册于 `novel-tools`）：**

| 工具 | 说明 |
|------|------|
| `GraphQuery` | summary / node / ready / pending_approval |
| `GraphAdvance` | Ready → Running + focus |
| `GraphSubmitForApproval` | submit + entrance gate（`human_intervened` / Auditor） |
| `GraphMarkVerified` | → Verifying |
| `GraphReopen` | REGATE + 可选 cascade |

**REGATE 解析：** `parse_regate_directive(text)` 匹配 `REGATE: <node_id>` + 可选 `REASON: ...`；fail-closed（id 不匹配则拒绝）。

### 1.9 Checkpoint

`GraphCheckpoint { loop_id, cursor, frozen_at, artifact_paths }` 存于 `knowledge/meta/checkpoints/{loop_id}.json`。供赛季冻结游标与产物清单。

---

## 2. 落盘文件

| 路径 | 内容 | 管理 |
|------|------|------|
| `knowledge/meta/plan-graph.json` | PlanGraph 契约 | `save_plan` / `load_plan` / `parse_plan` |
| `knowledge/meta/graph-state.json` | GraphState 运行时 | `save_state` / `load_state` |
| `knowledge/meta/graph.jsonl` | 事件日志（achieved / loop_advanced / loop_completed / demote_on_edit） | `append_jsonl` |
| `knowledge/meta/handoffs/{node_id}.json` | 节点最新 NodeHandoff | `save_handoff` |
| `knowledge/meta/handoffs/{node_id}-ch-{NNN}.json` | 按章归档 NodeHandoff | `save_handoff_chapter` / `list_handoff_chapters` |
| `knowledge/meta/checkpoints/{loop_id}.json` | 赛季冻结 | `save_checkpoint` / `load_checkpoint` |
