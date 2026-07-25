---
name: tauri-post-change
description: >-
  Tauri 壳（src-tauri/、novel-server/src/tauri/）修改后收尾：refactor-cleanup、
  command/event 注册走查、壳配置审查、然后跑 ci-tauri（check + build）。
  仅改 src-tauri/ 或 Tauri IPC 层且未触达 crates 业务逻辑时使用本 skill；跨层改动见 post-change-checklist。
---

# Tauri 壳修改后收尾

在 **novel_agent** 内修改 `src-tauri/**` 或 `crates/novel-server/src/tauri/**` 后执行。
若同时改了 `crates/` 业务逻辑或 `ui/` 前端，改用 [`post-change-checklist`](../post-change-checklist/SKILL.md)。

## 执行顺序（任一步改代码须从头重跑）

### 1. 代码清理（必做 · 读 [`refactor-cleanup`](../refactor-cleanup/SKILL.md)）

执行 [`refactor-cleanup`](../refactor-cleanup/SKILL.md) 全流程，聚焦 **Tauri 壳层**：

- 删除未注册的 `#[tauri::command]`
- 删除无 emit/listen 的事件定义
- 清理仅旧 UI 使用的 DTO 字段
- 跨层清理：删 Rust command/event 时同步搜 `ui/` 的 `invoke`/`listen`
- 注释与文档：与代码不一致时改写到反映当前行为

### 2. Command / Event 注册走查

1. **command 注册** — 所有 `#[tauri::command]` 在 `main.rs` 或 `lib.rs` 的 `.invoke_handler()` 中注册；`invoke` 命令名与 `ui/` 中字符串一致。
2. **event 注册** — 所有 `app_handle.emit(...)` / `emit_to(...)` 的事件名在 `ui/` 有对应 `listen(...)`；事件 payload 类型与 TS 类型一致。
3. **capabilities** — `src-tauri/capabilities/default.json` 权限与所用 API 匹配；新增 Tauri API 须更新 capabilities。
4. **生命周期** — `setup` 钩子中初始化的资源在 `drop`/`on_exit` 中释放；`spawn_event_forwarder` 等长生命周期 task 随 window 关闭终止。

### 3. 壳配置审查

| 配置项 | 位置 | 要点 |
|--------|------|------|
| 窗口 | `tauri.conf.json` → `app.windows` | 尺寸、resizable、title 与 UX 一致 |
| 安全 | `tauri.conf.json` → `app.security` | CSP 配置恰当；非必要不放开 |
| 构建 | `tauri.conf.json` → `build` | `frontendDist` 指向 `../ui/dist`；`beforeDevCommand` 正确 |
| 标识 | `tauri.conf.json` → `identifier` | `com.novelagent.app` |
| 图标 | `src-tauri/icons/` | `icon.png` + `icon.ico` 存在 |

### 4. Tauri 壳验证

```bash
bash scripts/ci-tauri.sh
```

等价 `ci-tauri-check`（`cargo check -p novel-agent`）+ `ci-linux-tauri-deps`（Linux 系统依赖）+ `cargo build -p novel-agent`。

**硬性要求：** check **0 warning**；build 通过。若仅改 `tauri.conf.json` / 图标 / capabilities，只跑本条即可，不必跑完整 `ci-rust-gate`。

### 5. 必跑：IPC 冒烟

改 `src-tauri/` 或 `novel-server/src/tauri/` 的 command/event 后**必须**跑 [`smoke-post-change`](../smoke-post-change/SKILL.md)：

```bash
bash scripts/smoke-ipc-fork.sh
.\scripts\smoke-ipc-fork.ps1
```

### 汇报模板

```markdown
## Tauri 壳收尾

- [x] 代码清理：<…>
- [x] Command/Event 注册：<一致 / 已修复>
- [x] 壳配置：<通过 / 已更新>
- [x] Tauri 验证：ci-tauri — 通过
- [x] IPC 冒烟：smoke-ipc-fork — Phase 1 通过 / Phase 2 <通过/跳过>
- [x] 文档已更新：<文件列表>
```
