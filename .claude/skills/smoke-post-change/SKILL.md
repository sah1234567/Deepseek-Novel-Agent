---
name: smoke-post-change
description: >-
  Novel Agent IPC fork flood prevention smoke test（非 CI，纯本地）。
  当修改 IPC 事件流、engine_loop、Fork/子 Agent drain、Tauri command/event 注册、
  或 ui/ 的 listen/emit 时自动触发。两阶段：Phase 1 自动化（nextest + vitest），
  Phase 2 可选手动 WebView2 验证或日志扫描。
---

# IPC Fork 防洪水冒烟测试

修改 **IPC 通道、Fork/子 Agent、engine 事件循环、Tauri command/event** 时必须执行本冒烟测试。
脚本：`scripts/smoke-ipc-fork.sh` (Unix) / `scripts/smoke-ipc-fork.ps1` (Windows PowerShell)。

## 何时触发

命中以下任一路径时自动触发（与 [`post-change-checklist`](../post-change-checklist/SKILL.md) 步骤 0 对齐）：

| 命中路径 | 原因 |
|----------|------|
| `crates/novel-core/src/engine_loop.rs` | 事件转发、子 Agent drain |
| `crates/novel-core/src/turn_loop.rs` | Fork 创建、interrupt |
| `crates/novel-server/src/tauri/` | command/event 注册、IPC 通道 |
| `src-tauri/` | Tauri 壳层事件桥接 |
| `ui/src/**/*.ts(x)` 中的 `listen(` / `emit(` | 前端事件订阅 |
| `scripts/smoke-ipc-fork.*` | 冒烟脚本本身 |

## Phase 1 — 自动化（必跑）

```bash
# Unix / Git Bash
bash scripts/smoke-ipc-fork.sh

# Windows PowerShell
.\scripts\smoke-ipc-fork.ps1
```

**Phase 1 覆盖（无需 WebView2）：**

| 测试集 | 位置 | 验证点 |
|--------|------|--------|
| `fork_stream` / `interruptible` / `fork_usage` / `gated` / `drain_subagent_jobs` | `crates/novel-core` (nextest) | Fork 创建、流式输出、中断、门控、子 Agent 清空 |
| `stream_coalesce` / `interruptible` / `clear_removes` | `crates/novel-server` (nextest, features=tauri) | IPC 合并、可中断、清理移除 |
| `ipc-flood-acceptance.test.ts` | `ui/` (vitest) | 前端洪水防护 |

**通过标准：** nextest **0 failed**、**0 SLOW/TIMEOUT**；vitest **0 failed**；无 DEPRECATED / esbuild 警告。

## Phase 2 — 手动（按需）

Phase 1 通过后，若改动涉及 **WebView2 实际渲染**（Tauri 事件 emit/listen、PostMessage 通道），需额外手动验证：

### Phase 2a — 日志扫描（有 dev 日志时）

```bash
bash scripts/smoke-ipc-fork.sh /path/to/tauri-dev-log.txt
.\scripts\smoke-ipc-fork.ps1 -LogFile C:\path\to\tauri-dev-log.txt
```

扫描目标：
- **FAIL:** `PostMessage failed` / `0x80070718`（IPC 配额耗尽）
- **WARN:** `engine_loop_exited`（非预期的 engine 退出——人工判断）

### Phase 2b — 手动 WebView2（无日志时）

1. `cargo tauri dev`
2. 主会话：并行触发 ForkSubAgent（2 个子 Agent），关闭 overlay；等待 drain 完成
3. 可选：打开一个 overlay → 确认实时流正常；关闭 → 流停止
4. 保存终端输出，用 Phase 2a 扫描

Phase 2 手动部分不可自动化；汇报时注明「Phase 2: SKIPPED（无日志文件）」或「Phase 2: PASSED」。

## 常见失败排查

| 症状 | 可能原因 | 修复方向 |
|------|---------|---------|
| PostMessage quota `0x80070718` | 事件发送过快，WebView2 队列满 | 检查 `stream_coalesce` 是否正确合并；降低 emit 频率 |
| `engine_loop_exited` 非预期出现 | engine drop 但 UI 仍在 listen | 检查 engine 生命周期与 `AppHandle` 引用计数 |
| Fork 子 Agent drain 超时 | 子 Agent 未正确终止 | 检查 `drain_subagent_jobs` 的 abort/complete 信号 |
| vitest `ipc-flood-acceptance` 失败 | 前端事件处理器未防抖/节流 | 检查 `listen` 回调中的 state 更新频率 |

## 与其他 skill 的关系

| Skill | 关系 |
|-------|------|
| [`post-change-checklist`](../post-change-checklist/SKILL.md) | 步骤 0 判定本 skill 是否需要运行 |
| [`tauri-post-change`](../tauri-post-change/SKILL.md) | Tauri 壳变更后联动触发 |
| [`rust-post-change`](../rust-post-change/SKILL.md) | engine_loop / turn_loop 变更后联动触发 |
| [`ui-post-change`](../ui-post-change/SKILL.md) | 前端事件变更后联动触发 |
