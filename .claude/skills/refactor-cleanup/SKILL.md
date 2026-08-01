---
name: refactor-cleanup
description: >-
  novel_agent 内每次代码修改或重构后自动执行：按 diff 触达层（Rust 后端 / ui 前端 / Tauri 壳 / DB·迁移）
  清理死代码、冗余兼容链路与过时注释；兼容收敛为迁移-on-read 或写明保留理由；注释与文档改写到与实现一致，禁止删注释掩盖不一致。
  被 rust-post-change、ui-post-change、tauri-post-change、post-change-checklist 等 skill 的步骤 1 绑定。
---

# 重构收尾：死代码 · 兼容层 · 注释对齐

在 **novel_agent** 内**每次**完成代码修改或重构后，对**本次 diff 触达的层**自动执行本清单。
由各层 post-change skill 的步骤 1 调用；也可独立用于纯清理任务。

## 触发时机

| 调用方 | 关系 |
|--------|------|
| [`rust-post-change`](../rust-post-change/SKILL.md) 步骤 1 | Rust 后端清理 |
| [`ui-post-change`](../ui-post-change/SKILL.md) 步骤 1 | UI 前端清理 |
| [`tauri-post-change`](../tauri-post-change/SKILL.md) 步骤 1 | Tauri 壳清理 |
| [`post-change-checklist`](../post-change-checklist/SKILL.md) 步骤 0 | 跨层编排（委托各层 skill 的步骤 1） |
| 独立调用 | 纯清理任务（无功能变更） |

仅改 `docs/`、`*.md`：核对与邻近实现是否一致；无则跳过。

## 按 diff 路径确定触达层

| 命中路径 | 清理侧重 |
|----------|----------|
| `crates/**`、`tests/integration/**` | Rust 模块、公共 API、工具/引擎逻辑 |
| `ui/**` | 组件、hooks、store、IPC 调用、类型 |
| `src-tauri/**`、`novel-server/src/tauri/**` | command/event/DTO、壳配置 |
| `crates/novel-state/**`、DB 迁移、metadata 字段 | schema、迁移、读写 API、legacy 行格式 |
| 跨层（IPC / 事件名 / DTO 字段） | **同时**扫发送方与接收方，禁止只删一侧 |

---

## 1. 扫描发现（先搜后改）

用以下命令定位死代码、兼容线索、过时引用：

```bash
# 全仓库引用
rg -n "YourSymbol" crates/ ui/ src-tauri/ docs/

# 死代码 / 兼容标记
rg -n "dead_code|deprecated|legacy|backward|compat|allow\\(dead" crates/ ui/
rg -n "TODO.*remove|FIXME.*compat" crates/ ui/

# IPC 契约（改 command/event 时必做）
rg -n "invoke\\(|listen\\(|emit\\(" ui/
rg -n "tauri::command|#\\[command" src-tauri/ crates/novel-server/src/tauri/

# DB / metadata
rg -n "metadata_json|get_session_|set_session_" crates/
```

---

## 2. 死代码（删除）

### 识别信号

| 信号 | 动作 |
|------|------|
| `rg`/IDE 无引用（除定义与测试） | 删符号、文件、export |
| 一行转发/别名，无额外语义 | 内联后删除 |
| 同一知识重复实现 | 合并为单一 helper |
| 仅为旧路径的 re-export / wrapper | 删包装，调用方改直引 |
| 未使用的 import、常量、类型、测试桩 | 删除 |
| 注释掉的大块旧代码 | 删除（git 追溯，不留源码） |

### 按层典型线索

| 层 | 线索 |
|----|------|
| **Rust** | `allow(dead_code)`、`#[deprecated]`、仅测试用的 `pub fn`、未再 re-export 的过渡 API |
| **前端** | 未引用组件/hook/store；`export` 无 importers；未使用类型与常量 |
| **Tauri** | 未注册的 `#[tauri::command]`；无 emit/listen 的事件；DTO 字段仅旧 UI 用 |
| **DB** | 读后从不写的 metadata 键；迁移后仍保留的旧表读写；双写其中一路已无消费者 |

### 不应删除的情况

| 信号 | 处理 |
|------|------|
| 函数有 `#[allow(dead_code)]` 但注释说明了保留原因 | 保留；确认注释清晰 |
| `pub` 导出被外部 crate 或未扫描目录使用 | 保留（`rg` 作用域有限） |
| 用户明确标记「勿动」的代码 | 保留 |

---

## 3. 向后兼容（收敛，非永久堆叠）

### 处理决策

| 真实数据/调用方是否仍需要？ | 处理 |
|--------------------------|------|
| 否（可 reset / 仅 dev） | 删除兼容分支，只保留当前契约 |
| 是，但仅需首次接触一次 | **迁移-on-read**：读时推断 → 写回 canonical → 之后只读新路径 |
| 是，且必须每次动态计算 | 保留；注释写清触发条件与数据形态；不标为「默认路径」 |

### 迁移-on-read 模式

**后端 / DB：** 读 canonical → 缺失时从 legacy 推断 → 写回 → 之后只读 canonical。
`infer_*` 保持私有；注释标明「仅缺字段的旧 session / 旧行，首次打开一次」。

**前端 / IPC：** 旧 localStorage 键 / 旧 event payload：读取一次 → 写入新 shape → 删旧键。
UI 与 Tauri 命令/DTO **同 PR** 切换；不长期双发双收。

**DB 专项：** 新字段进 `metadata_json` 或新列：写入路径与 resume/列表读取**必须一致**。
已 `DROP` 的表不再有读写代码。
格式异常：用 `reset-work-databases` + 新建 session，不在代码里堆无限兼容。

---

## 4. 注释与文档（改写到一致，禁止删除掩盖）

> **注释与代码逻辑因修改而不一致时，必须更新注释以反映当前行为；严禁通过删除注释来消除不一致。**

| 过时描述 | 改为 |
|----------|------|
| 已删除的函数/命令/事件名 | 当前 API 名与调用链 |
| 「多级 fallback 每次执行」 | 「读 canonical；缺失时迁移写回一次」 |
| 旧返回值/字段列表 | 与当前类型定义一致 |
| 「前端轮询 X」但已改 push 事件 | 与现 IPC 模型一致 |

适用范围：源码注释（`//`、`///`、`/** */`）、command/event 旁注、`docs/crates/*.md`、`.claude/skills/*.md`。

---

## 5. 验证

与各层 post-change skill 的验证步骤对齐——**只跑触达层**：

| 层 | 命令 |
|----|------|
| **A. Rust 后端** | `bash scripts/ci-rust-gate.sh`（内部含 fmt/clippy/nextest/tauri/CRAP 全链，见 CLAUDE.md CI Gates；单步调试可用 ci-rust-static / ci-clippy / ci-rust-test） |
| **B. Tauri 壳** | `bash scripts/ci-tauri.sh` |
| **C. 前端** | `bash scripts/ci-frontend.sh` |
| **DB / 迁移** | 相关 crate nextest；**本次 diff 含迁移文件或新 metadata 字段时**手工验证迁移 + metadata roundtrip |
| **CRAP** | 改 `crates/` 生产代码后必跑：`ci-lcov` → `ci-crap`（见 [`cargo-crap`](../cargo-crap/SKILL.md)） |
| **冒烟** | 触及 IPC/Fork/事件流：见 [`smoke-post-change`](../smoke-post-change/SKILL.md) |

**禁止 `cargo test`。**

---

## 6. 汇报模板

```markdown
## 重构清理报告

**触达层：** A 后端 / B Tauri / C 前端 / DB（勾选）

### 删除
- `path` — 无引用 / 冗余包装 / 未使用 export

### 兼容层
- `…` — 迁移-on-read 写回 metadata / 保留，边界：…

### 注释/文档
- `path`：从「…」改为「…」

### 验证
- A：ci-rust-gate — 通过 / 跳过（未触达 crates 生产代码）
- B：ci-tauri — 通过 / 跳过
- C：ci-frontend — 通过 / 跳过
- CRAP：通过 / 跳过（未改 crates 生产代码）
- 冒烟：smoke-ipc-fork — 通过 / 跳过

### 清单
- [ ] 每层死代码/兼容/注释已处理或说明保留理由
- [ ] 跨层 IPC/DTO/metadata 两端一致
- [ ] 触达层验证已跑通过；未触达层标「跳过」
- [ ] 无新增 `allow(dead_code)` / `@ts-ignore`
- [ ] 相关 docs/ skill 已改写到与实现一致
```

---

## 附录：novel_agent 参考示例

<details>
<summary>Rust permission 模块</summary>

| 删除项 | 原因 |
|--------|------|
| `format_mode_transition_prefix` | 仅测试 + `allow(dead_code)` |
| `transition_plan_to_prefix` | 一行转发 |
| `PermissionModeSource` | 仅日志；迁移-on-read 后不再需要 |

兼容：`resolve_session_permission_mode` 缺 metadata 时推断并 `set_session_permission_mode` 写回。

</details>

<details>
<summary>DB metadata</summary>

- `metadata_json.permission_mode`：canonical；勿永久从消息 infer
- 已移除：`api_config` 表、旧 session 无 `system_static_frozen`（见 `require_frozen_system_metadata` 错误提示）

</details>

<details>
<summary>跨层 IPC</summary>

改 Tauri command 或 event 名时：同时搜 `ui/` 的 `invoke`/`listen` 与 `novel-server/src/tauri/` 注册处；删 DTO 字段前确认 `ui` 类型一致。

</details>
