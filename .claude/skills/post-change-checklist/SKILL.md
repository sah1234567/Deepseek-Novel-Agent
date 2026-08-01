---
name: post-change-checklist
description: >-
  Novel Agent 跨层修改后收尾编排器。当改动同时触达 Rust 后端 + UI 前端 + Tauri 壳
  （如 IPC 契约、DTO 字段、事件名），按 diff 范围串行调度各层 skill：
  rust-post-change → tauri-post-change → ui-post-change → smoke-post-change。
  仅单层改动：直接用对应层的 skill（更快，无需全量）。
---

# 跨层修改收尾编排器

当改动**同时触达多个层**（如改 IPC 契约同时涉及 `crates/` + `ui/` + `src-tauri/`）时使用本编排器。
**仅单层改动：直接用对应 skill**（更快、更聚焦）：

| 改动范围 | 直接用 |
|----------|--------|
| 仅 `crates/**`（Rust 后端逻辑） | [`rust-post-change`](../rust-post-change/SKILL.md) |
| 仅 `ui/**`（前端） | [`ui-post-change`](../ui-post-change/SKILL.md) |
| 仅 `src-tauri/**`、`novel-server/src/tauri/**` | [`tauri-post-change`](../tauri-post-change/SKILL.md) |
| IPC/Fork/engine 事件 | [`smoke-post-change`](../smoke-post-change/SKILL.md) |
| 纯清理/重构 | [`refactor-cleanup`](../refactor-cleanup/SKILL.md) |

## 步骤 0：判定触发层（必做）

根据 git diff 路径勾选：

| 层 | 典型命中路径 | 对应 skill |
|----|-------------|------------|
| **A. Rust 后端** | `crates/**`、`tests/integration/**`；改 `Cargo.toml` / `Cargo.lock`（workspace 依赖） | [`rust-post-change`](../rust-post-change/SKILL.md) |
| **B. Tauri 壳** | `src-tauri/**`、`novel-server/src/tauri/**` | [`tauri-post-change`](../tauri-post-change/SKILL.md) |
| **C. UI 前端** | `ui/**` | [`ui-post-change`](../ui-post-change/SKILL.md) |
| **S. IPC 冒烟** | 上述任一层涉及 Fork/子 Agent/事件流/command/event 注册 | [`smoke-post-change`](../smoke-post-change/SKILL.md) |

## 执行顺序

按依赖关系串行：**A → B → C → S**。跳过的层在汇报中标「未触达，跳过」。

```
A (rust-post-change) → B (tauri-post-change) → C (ui-post-change) → S (smoke-post-change)
```

跨层改动重点：**改 IPC 契约（command/event/DTO 字段）时，各层 skill 的链路走查会自动核对两端一致性。** 若任一层改代码，回到该层 skill 的步骤 1 重跑。

## 依赖审计（可选）

改 `Cargo.toml` / `Cargo.lock` 时建议加跑：

```bash
bash scripts/ci-security-audit.sh
```

网络失败时 `SKIP_SECURITY_AUDIT=1`（发 PR 以 CI 为准）。

## 文档同步

跨层改动最后更新（各层 skill 内部也会提示更新对应文档）：

| 改动范围 | 更新目标 |
|----------|----------|
| crate API/行为 | `docs/crates/<crate>.md`（12 个 crate 一一对应） |
| Graph 编排 / plan schema / Loop | `docs/crates/novel-graph.md` |
| Agent 循环 / Fork / 流式 Tool | `docs/crates/novel-core.md` |
| LLM 客户端 / SSE / tool_args | `docs/crates/novel-deepseek.md` |
| 工具注册 / 查询工具族 | `docs/crates/novel-tools.md` |
| 记忆类型 / 提取 / prefetch | `docs/crates/novel-memory.md` |
| 路径 / API json / AppConfig | `docs/crates/novel-config.md` |
| 作品 IPC / AppStatus | `docs/crates/novel-server.md`、`FRAMEWORK.md` |
| IPC 命令/事件契约 | `FRAMEWORK.md` §2.5.1 |
| 知识契约 / 审计落盘 / findings | `docs/crates/novel-knowledge.md` §1.1.2 |
| scaffold / templates | `docs/crates/novel-knowledge.md` |
| compaction / metadata / DB sync | `docs/crates/novel-state.md`、`novel-compaction.md` |
| tracing / 审计日志 | `docs/crates/novel-logging.md` |
| Skill 规则 | `skills/<id>/` + `docs/crates/novel-skills.md` |
| 能力概览 | `README.md`、`docs/README.md` |

## 汇报模板

```markdown
## 跨层收尾结果

### 步骤 0 判定
- A (Rust 后端): <勾选 / 未触达>
- B (Tauri 壳): <勾选 / 未触达>
- C (UI 前端): <勾选 / 未触达>
- S (IPC 冒烟): <勾选 / 未触达>

### 各层结果
- [x] A — rust-post-change：<见该 skill 汇报 / 跳过>
- [x] B — tauri-post-change：<见该 skill 汇报 / 跳过>
- [x] C — ui-post-change：<见该 skill 汇报 / 跳过>
- [x] S — smoke-post-change：<见该 skill 汇报 / 跳过>
- [x] 依赖审计（可选）：<通过 / 跳过>

### 文档
- [x] 已更新：<文件列表>
```
