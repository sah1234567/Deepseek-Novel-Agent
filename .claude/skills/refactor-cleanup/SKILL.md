---
name: refactor-cleanup
description: >-
  novel_agent 内每次代码修改或重构后自动执行：按 diff 触达层（Rust 后端 / ui 前端 / Tauri 壳 / DB·迁移）
  清理死代码、冗余兼容链路与过时注释；兼容收敛为迁移-on-read 或写明保留理由；注释与文档改写到与实现一致，禁止删注释掩盖不一致。
  与 post-change-checklist 步骤 1 绑定。
---

# 重构收尾：死代码 · 兼容层 · 注释对齐

在 **novel_agent** 内**每次**完成代码修改或重构后，对**本次 diff 触达的层**自动执行本清单（[`post-change-checklist`](../post-change-checklist/SKILL.md) 步骤 1）。仅改文档且无逻辑变更时可快速过表并标「无代码清理项」。

## 触发时机（自动）

**任意逻辑改动提交前必跑**。按 diff 路径确定触达层：

| 命中路径 | 清理侧重 |
|----------|----------|
| `crates/**`、`tests/integration/**` | Rust 模块、公共 API、工具/引擎逻辑 |
| `ui/**` | 组件、hooks、store、IPC 调用、类型 |
| `src-tauri/**`、`novel-server/src/tauri/**` | command/event/DTO、壳配置 |
| `crates/novel-state/**`、DB 迁移、metadata 字段 | schema、迁移、读写 API、legacy 行格式 |
| 跨层（IPC / 事件名 / DTO 字段） | **同时**扫发送方与接收方，禁止只删一侧 |

仅改 `docs/`、`*.md`：核对与邻近实现是否一致；无则跳过。

---

## 1. 扫描发现（先搜后改）

用以下命令定位死代码、兼容线索、过时引用。将 `YourSymbol` 替换为本次 diff 涉及的符号名：

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

### 操作步骤

1. **全仓库搜索**符号名、字符串常量、IPC 命令名、事件名、`metadata_json` 键名。
2. **删跨层项前**确认对端无引用（删 Rust DTO 字段→搜 `ui/`；删 `invoke` 命令→搜 `src-tauri` + `ui/`）。
3. **同 PR 更新所有调用方**；不留无截止日期的 deprecated 别名。
4. 删后按 §5 跑触达层验证。

### 不应删除的情况

| 信号 | 处理 |
|------|------|
| 函数有 `#[allow(dead_code)]` 但注释说明了保留原因 | 保留；确认注释清晰 |
| `pub` 导出被外部 crate 或未扫描目录使用 | 保留（`rg` 作用域有限） |
| 用户明确标记「勿动」的代码 | 保留 |

---

## 3. 向后兼容（收敛，非永久堆叠）

### 识别信号

- 永久多级 fallback（每次请求都走旧推断链）
- 双路径完成同一事（旧 infer + 新 metadata / 旧 IPC + 新 command）
- 注释写「legacy」但无迁移写回——旧逻辑每次重复执行
- 前端同时支持旧事件名与新事件名，但后端只发一种
- DB 读旧列却不迁移，或迁移后仍保留无限期双读

### 处理决策

| 真实数据/调用方是否仍需要？ | 处理 |
|--------------------------|------|
| 否（可 reset / 仅 dev） | 删除兼容分支，只保留当前契约 |
| 是，但仅需首次接触一次 | **迁移-on-read**：读时推断 → 写回 canonical → 之后只读新路径 |
| 是，且必须每次动态计算 | 保留；注释写清触发条件与数据形态；不标为「默认路径」 |

### 迁移-on-read 模式

**后端 / DB：**

```rust
// 读 canonical → 缺失时从 legacy 推断 → 写回 → 之后只读 canonical
if let Some(v) = db.get_canonical(id)? { return Ok(v); }
let v = infer_legacy(...).unwrap_or_else(|| default(...));
db.set_canonical(id, &v)?;
Ok(v)
```

- `infer_*` 保持私有；注释标明「仅缺字段的旧 session / 旧行，首次打开一次」
- 测试：断言写回后二次读取不再走 infer
- 删除仅服务永久 infer 的 `Source` 枚举、重复 fallback 链

**前端 / IPC：**

- 旧 localStorage 键 / 旧 event payload：读取一次 → 写入新 shape → 删旧键
- UI 与 Tauri 命令/DTO **同 PR** 切换；不长期双发双收

**DB 专项：**

- 新字段进 `metadata_json` 或新列：写入路径与 resume/列表读取**必须一致**
- 已 `DROP` 的表不再有读写代码
- 格式异常：用 `reset-work-databases` + 新建 session，不在代码里堆无限兼容

### 应保留的边界逻辑

当前行为依赖的分支（如清除陈旧 pending 前缀、子 Agent 临时模式不写 metadata）**保留**，注释写清触发条件，与「废弃兼容链」区分。

---

## 4. 注释与文档（改写到一致，禁止删除掩盖）

### 硬性规则

> **注释与代码逻辑因修改而不一致时，必须更新注释以反映当前行为；严禁通过删除注释来消除不一致。**

### 处理优先级

1. **改**：与实现、测试、IPC、DB 契约一致
2. **补**：非显而易见的业务规则（为何不写 DB、为何 Clear）
3. **删**：**仅当**描述对象已从代码库移除（非掩盖不一致）

### 过时描述→改法

| 过时 | 改为 |
|------|------|
| 已删除的函数/命令/事件名 | 当前 API 名与调用链 |
| 「多级 fallback 每次执行」 | 「读 canonical；缺失时迁移写回一次」 |
| 旧返回值/字段列表 | 与当前类型定义一致 |
| 「前端轮询 X」但已改 push 事件 | 与现 IPC 模型一致 |

适用范围：源码注释（`//`、`///`、`/** */`）、command/event 旁注、`docs/crates/*.md`、`.claude/skills/**`、`prompt/**`（若行为变更）。不自动改用户标记勿动的 plan 文件。

---

## 5. 验证

与 [`post-change-checklist`](../post-change-checklist/SKILL.md) 步骤 0 对齐——**只跑触达层**：

| 层 | 命令 |
|----|------|
| **A. Rust 后端** | `cargo fmt --all -- --check`；`bash scripts/ci-clippy.sh`；`cargo nextest run -p <crate> --profile ci` |
| **B. Tauri 壳** | `bash scripts/ci-tauri.sh` |
| **C. 前端** | `bash scripts/ci-frontend.sh` |
| **DB / 迁移** | 相关 crate nextest；必要时手工验证迁移 + metadata roundtrip |
| **可选 CRAP** | 改 `crates/` 后：`ci-lcov` → `ci-crap`（见 `cargo-crap` skill；Windows 用 `.ps1`） |

全量 Rust 验证用 `bash scripts/ci-rust-test.sh`（workspace nextest）。**禁止 `cargo test`**。

---

## 6. 汇报与收尾勾选

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
- A：fmt / clippy / nextest — 通过 / 跳过
- B：ci-tauri — 通过 / 跳过
- C：ci-frontend — 通过 / 跳过

### 清单
- [ ] 每层死代码/兼容/注释已处理或说明保留理由
- [ ] 跨层 IPC/DTO/metadata 两端一致
- [ ] 触达层验证已跑通过；未触达层标「跳过」
- [ ] 无新增 `allow(dead_code)` / `@ts-ignore`
- [ ] 相关 docs/ skill 已改写到与实现一致
```

---

## 7. 与相邻 skill 的关系

| Skill | 关系 |
|-------|------|
| [`post-change-checklist`](../post-change-checklist/SKILL.md) | 本 skill = 步骤 1；步骤 0 定层，步骤 2-3 走查审查，步骤 4-7 跑 CI |
| [`engineering-principles`](../engineering-principles/SKILL.md) | 拆分与最小暴露指导清理方向 |
| [`cargo-crap`](../cargo-crap/SKILL.md) | 仅 Rust 后端可选分析 |

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
