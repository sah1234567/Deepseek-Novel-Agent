---
name: workflow-design
description: 工作流设计参考 — 节点标签目录、Loop 模式、PlanBuilder 使用范例
when_to_use: 新建作品建图、调整工作流结构时使用
skill_kind: reference
allowed-tools: PlanBuilder, GraphQuery, Read, InvokeSkill, AskUserQuestion
---

# 工作流设计

## 推荐节点 Tags（长篇小说）

| Tag | 说明 | 典型 artifacts |
|-----|------|---------------|
| `world_bible` | 世界观与人物设定 | knowledge/shared-systems/, knowledge/characters/ |
| `outline` | 大纲规划 | knowledge/plot/大纲.md |
| `chapter_body` | 章节正文写作 | chapters/chapter-NNN.md |
| `sync_canon` | 正典同步 | knowledge/ 各追踪文件 |
| `volume_review` | 卷级统稿 | knowledge/meta/volume-review.md |
| `batch_review` | 批量审查 | 审计报告 |
| `ensure_fine_outline` | 细纲确保 | knowledge/plot/细纲/ |

## 推荐 Loop 模式

### Chapter Loop（逐章写作）
```
stations: [ensure-outline, write-chapter, sync-canon]
cursor.counters: { chapter: 1, round: 1 }
advance: { increment: "chapter", step: 1, side_effects: [{op:"increment", counter:"round", by:1}] }
until: { type: "counter_gt", counter: "chapter", value_from: "targetChapters" }
advance_after: sync-canon
```

### Batch Review Loop（每 N 章审查）
在 Chapter Loop 基础上增加 `batch-review` 节点作为 advance_after，cursor 增加 `batch` counter。

### Volume Loop（卷级循环）
```
cursor.counters: { volume: 1, chapter: 1 }
advance: { increment: "volume", step: 1, side_effects: [{op:"reset", counter:"chapter"}] }
until: { type: "counter_gt", counter: "volume", value: 5 }
```

## PlanBuilder 使用范例

### 构建 Chapter Loop

1. `PlanBuilder(operation="add_node", node_id="outline", title="大纲", spec="...", tags=["outline"])`
2. `PlanBuilder(operation="add_node", node_id="write-chapter", title="写章", spec_template="write ch{{cursor.chapter}}", deps=["outline"], tags=["chapter_body"])`
3. `PlanBuilder(operation="add_node", node_id="sync-canon", title="正典同步", deps=["write-chapter"], tags=["sync_canon"])`
4. `PlanBuilder(operation="add_loop", loop_id="book-body", stations=["outline","write-chapter","sync-canon"], entry="outline", advance_after="sync-canon", cursor={"counters":{"chapter":1,"round":1}}, advance={"increment":"chapter","step":1,"side_effects":[{"op":"increment","counter":"round","by":1}]}, until={"type":"counter_gt","counter":"chapter","value_from":"targetChapters"})`
5. `PlanBuilder(operation="set_dep", node_id="write-chapter", depends_on="outline")`
6. `PlanBuilder(operation="preview")` → 确认无误
7. `PlanBuilder(operation="commit")` → 落盘

## 节点验收配置参考

- `world_bible`: acceptance.human.required = true
- `outline`: acceptance.machine = "auditor", human.required = true
- `chapter_body`: iterate.max_iterations = 8, iterate.until = "auditor_pass"
- `sync_canon`: acceptance.machine = "none"
