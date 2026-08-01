---
name: ui-post-change
description: >-
  UI 前端（ui/）修改后收尾：refactor-cleanup、IPC 链路走查、前端反模式审查、
  然后跑 ci-frontend（pnpm audit + test + build）。
  仅改 ui/ 且未触达 crates/ 或 src-tauri/ 时使用本 skill；跨层改动见 post-change-checklist。
---

# UI 前端修改后收尾

在 **novel_agent** 内修改 `ui/**` 后执行。
若同时改了 `crates/` 或 `src-tauri/`（IPC 契约/command/event），改用 [`post-change-checklist`](../post-change-checklist/SKILL.md)。

**环境：** Node **24**（`ui/.nvmrc`；`ci-check-node.sh` 在校验步骤前检查）。Windows 用 Git Bash。

## 执行顺序（任一步改代码须从头重跑）

### 1. 代码清理（必做 · 读 [`refactor-cleanup`](../refactor-cleanup/SKILL.md)）

执行 [`refactor-cleanup`](../refactor-cleanup/SKILL.md) 全流程，聚焦 **前端层**（`ui/**`）：

- 删除未引用组件/hook/store/类型/常量
- 删除仅旧 IPC 使用的 `invoke`/`listen` 调用
- 清理未使用的 import、`@ts-ignore`、注释掉的旧代码
- 注释与文档：与代码不一致时改写到反映当前行为；严禁删注释掩盖不一致

### 2. IPC 链路走查

仅检查改动实际触达的部分；未涉及的项标注「不适用」。

1. **主路径** — 核对 `invoke` 命令名与 Rust 侧 `#[tauri::command]` 注册一致；`listen`/`emit` 事件名与 `Event` 枚举一致。`AppStatus` 含 `activeWorkName`/`projectRoot`。
2. **Transcript / 懒加载** — `EVICT_TURNS` 仅卸 FSM（`TAIL_LOADED_TURNS` 6 贴底、`VIEW_LOADED_TURNS` 6 浏览窗口、`MAX_LOADED_TURNS` 18 硬顶）；`planMemoryReconcile` 统一预取/淘汰；`compactionPaused` 不收缩。
3. **类型镜像** — TS `protocol.ts` / DTO 类型与 Rust struct 字段对齐；新增字段需两端同步。
4. **数据归属** — API Key 不写入 per-work DB 或 `settings.json`；`invoke` 参数不含敏感信息。

### 3. 前端反模式审查

| 反模式 | 禁止 | 推荐 |
|--------|------|------|
| 忽略 Promise | `async fn` 调用不加 `await` 或 `.catch()` | 显式处理或注释说明有意 fire-and-forget |
| `any` / `@ts-ignore` | 新增 `any` 类型、`@ts-ignore` 注释 | 定义具体类型；`unknown` + type guard |
| 多次 WS / 直连 | 组件直接 `new WebSocket()` | 统一走 Tauri `invoke`/`listen`（`ipc/commands.ts` + `ipc/events.ts` 常量，监听集中在 `useAgentTauriListeners`） |
| 巨型组件 | 一个组件管理数据加载 + 事件监听 + UI 渲染 | 逻辑抽到 hook，组件只渲染（见 [`engineering-principles`](../engineering-principles/SKILL.md)） |
| Props 透传 | 透传整个 domain 对象 | Props 只声明实际需要的字段（最小暴露） |
| 配置散落 | 硬编码 URL/端口/超时 | 统一从 `AppConfig` 或 env 读取 |

### 4. 前端验证

```bash
bash scripts/ci-frontend.sh
```

等价 `ci-check-node.sh` + `pnpm install --frozen-lockfile` + `pnpm audit --audit-level=critical` + `pnpm test` + `pnpm run build`。

**硬性要求：** **0 failed**；`pnpm audit` 无 critical；无 ERROR / DEPRECATED / vitest·esbuild 警告。

**测试补写：** 单元测 `ui/src/**/*.test.ts(x)`；fixture `ui/src/test/`。改 transcript / 聊天 UI 时尽可能覆盖流式 tool、ReAct、AskUserQuestion、Write 批准、Fork、INTERRUPT、MERGE_TURNS / Turn 懒加载。仅样式可标「无新增场景」，仍须 `pnpm test` 全绿。

### 5. 可选：IPC 冒烟

若改动触及前端 `listen`/`emit` 或 IPC 事件流，见 [`smoke-post-change`](../smoke-post-change/SKILL.md)。

### 文档同步

改 IPC/类型/事件后按改动范围更新：`docs/crates/novel-server.md`、`FRAMEWORK.md` §2.5.1（命令/事件契约）；能力概览 → `README.md`、`docs/README.md`。

### 汇报模板

```markdown
## UI 前端收尾

- [x] 代码清理：<…>
- [x] IPC 链路走查：<摘要>
- [x] 前端反模式：<通过 / N 处已修>
- [x] 前端验证：ci-frontend — 通过
- [x] 测试补写：<已补 / 不适用>
- [x] 文档已更新：<文件列表>
```

### 关联 Skill

| Skill | 用途 |
|-------|------|
| [`refactor-cleanup`](../refactor-cleanup/SKILL.md) | 步骤 1 委托：死代码/兼容层/注释清理 |
| [`smoke-post-change`](../smoke-post-change/SKILL.md) | 步骤 5 可选：listen/emit 改动时 IPC 冒烟 |
| [`engineering-principles`](../engineering-principles/SKILL.md) | 审查基准：TS 反模式与组件设计原则 |
| [`post-change-checklist`](../post-change-checklist/SKILL.md) | 跨层改动时改用本编排器 |
