---
name: post-change-checklist
description: >-
  Novel Agent 修改后收尾：清理、Tracing、链路走查、Rust 审查；再按 diff 范围**分别**跑后端 / Tauri 壳 / 前端验证（对应 scripts/ci-*.sh，未触达可跳过）；
  可选 cargo audit；Skill 与文档同步。在 novel_agent 内完成改动后自动执行。
---

# 修改后收尾清单

在 **novel_agent** 内完成改动后：**先**完成代码稳定（步骤 1→3），**再**按本次 diff 判定要跑哪几条验证（步骤 4–7 可跳过未触达部分）。全部**已执行**项通过后汇报。

**环境：** 与 GitHub CI 相同脚本，见 [`scripts/README.md`](../../../scripts/README.md)。Windows 用 **Git Bash**（`.\scripts\ci-windows.ps1` 或 `bash scripts/…`），勿用 WSL `bash`。Node **24**（`ui/.nvmrc`；`ci-check-node.sh` 在 npm 步骤前校验）。

**阶段划分**

| 阶段 | 步骤 | 说明 |
|------|------|------|
| 代码稳定 | 1 → 3 | 清理、Tracing、链路走查、Rust 审查；**任一步改代码须从步骤 1 重跑** |
| 验证闸门 | **4 → 7** | **按范围择一或多条**：后端 → Tauri 壳 → 前端 →（可选）audit；**未触达的整条跳过** |
| 同步交付 | 8 → 9 | Skill / 文档，**放最后** |

## 0. 判定本次要跑哪几条验证（必做）

根据 **git diff 路径**（或等效文件列表）勾选；**只跑勾中的条**，其余在汇报中标「未触达，跳过」。

| 范围 | 典型路径（命中任一即勾选） | 脚本（按序执行） | 通过标准 |
|------|---------------------------|------------------|----------|
| **A. 后端** | `crates/**`、`tests/integration/**`；改 `Cargo.toml` / `Cargo.lock`（workspace 依赖） | ① `bash scripts/ci-rust-static.sh`（**fmt + check**）<br>② `bash scripts/ci-clippy.sh`（**clippy**）<br>③ `bash scripts/ci-rust-test.sh`（**nextest**） | fmt 无 diff；check/clippy **0 warning**；nextest **0 failed**、**0 SLOW/TIMEOUT**（ignored 除外） |
| **B. Tauri 壳** | `src-tauri/**`；`novel-server/src/tauri/**` 仅当需编过 `novel-agent` 二进制 | `bash scripts/ci-tauri.sh`（**check + build** `novel-agent`；内含 `ci-tauri-check` + `ui/dist`） | Tauri 壳编译通过 |
| **C. 前端** | `ui/**` | `bash scripts/ci-frontend.sh`（**npm audit critical + test + build**） | **0 failed**；`npm audit` 无 critical；无 ERROR / DEPRECATED / vitest·esbuild 警告 |
| **D. 依赖 audit**（可选） | 改 `Cargo.toml` / `Cargo.lock`；发 PR 前与 CI 对齐 | `bash scripts/ci-security-audit.sh` | `cargo audit --deny warnings` 通过（Tauri 传递依赖见 `.cargo/audit.toml`）；网络失败见脚本提示或 `SKIP_SECURITY_AUDIT=1` |

**组合规则（避免多跑）：**

- **只改 `crates/novel-core/`** → 仅 **A**；不跑 B/C。
- **只改 `ui/src/`** → 仅 **C**；不跑 A/B（除非同时改了 `src-tauri` 或 crates）。
- **只改 `src-tauri/tauri.conf.json`** → 仅 **B**（会拉 `ui/dist` 并编壳）；不跑 A 的 nextest，除非也改了 crates。
- **IPC：同时改 `ui/` 事件名 + `novel-server/.../tauri/`** → **C + B**；若 `novel-server` 或 `crates` 有逻辑改动再加 **A**。
- **改 workspace 依赖（根或某 crate 的 `Cargo.toml`）** → **A**；建议加 **D**。

**全量 CI（可选）：** 发 PR 前可跑 `.\scripts\ci-windows.ps1` 或 `bash scripts/ci-pr-gate.sh`（= A+B+C+D + 前端已含在 gate 内），等价 GitHub；日常收尾仍推荐按上表**只跑触达范围**。

改逻辑须在步骤 2 先补缺失用例，再跑对应验证。仅文档/注释：验证 4–7 均可标「不适用」。

开发中可单独 `cargo check -p <crate>` 快速反馈；**收尾通过标准以勾选项的脚本为准**。

**数据归属（走查锚点）**

| 资源 | 路径 | 作用域 |
|------|------|--------|
| 会话 DB | `works/{名}/.novel-agent/state.db` | 每作品独立 |
| WebSearch 原始缓存 | `works/{名}/.websearch/` | 每作品；非 `knowledge/` 正典 |
| 作品配置 | `works/{名}/settings.json` | 每作品 |
| 全局 API | `{agent_root}/.novel-agent/api_config.json` | Agent 级（env 优先） |
| 运行时 Skill | `{agent_root}/skills/`（Agent 级）+ `works/{name}/skills/`（project 级覆盖） | 二级加载，project 覆盖 Agent |
| 脚手架模板 | `{agent_root}/templates/` | 运行时读盘，必填 |

Agent 根目录无统一 SQLite。API Key **不**写入 per-work DB 或 `settings.json`。废弃：`NOVEL_API_KEY`、`NOVEL_PROJECT_ROOT`、`create_session(project_root)`。

## 1. 代码清理

清理本次改动产生的废弃内容，**除非用户明确要求保留**：

- 已无调用方的函数、模块、配置项、分支
- 仅为旧行为服务的向后兼容逻辑、适配层、deprecated 别名（`state.db` 的 `api_config` 表已移除，勿写旧路径）
- 因重构而闲置的 import、常量、测试桩

注释与代码逻辑因修改而不一致时，**必须更新注释以反映当前行为**；**严禁通过删除注释来消除不一致**。

## 1.5 Tracing 埋点（必做）

改业务逻辑须补可观测性，但 **dev 终端要短**：默认 `info` 只打 turn 边界与失败；其余用 `debug`。细则见 [`docs/crates/novel-logging.md`](../../../docs/crates/novel-logging.md)。

| 级别 | 用途 |
|------|------|
| `info!` | `turn_start` / `turn_complete`（仅 `turn` + `reason`）；压缩起止 |
| `warn!` / `error!` | LLM/工具/DB 失败、校验拒绝、circuit breaker |
| `debug!` | IPC、LLM 请求、每笔 tool、落库、token、续跑/子 Agent |
| `trace!` | 高频轮询（`GetStatus`） |

**禁止** API Key、tool 全文进 log。里程碑写 `audit_log`（Turn/Tool/Token/Error/Compaction 等，见 novel-logging.md）。纯文档/重命名可标「Tracing：不适用」。深度排查：`RUST_LOG=novel_core=debug cargo tauri dev`。

## 2. 链路走查

在不运行程序的前提下，通读本次 diff，沿代码链路逐段核对。**仅检查改动实际触达的部分**；未涉及的项标注「不适用」，并简要说明原因。每条结论须注明依据（`路径:行号`）。若修改了代码，从步骤 1 重新开始。

先判断改动属于以下哪几类（不属于的整段跳过）：

- **IPC / 前端**：`ui/`、`src-tauri/`、`novel-server/src/tauri/`
- **配置 / 作品**：`novel-config/`（paths、agent_config）、`AppConfig`、`main.rs` setup
- **Turn / 工具**：`novel-core/`、`novel-tools/`
- **状态 / 知识库**：`novel-state/`、`novel-knowledge/`（含 scaffold、templates）
- **Compaction / Prompt**：`novel-compaction/`、`prompt/`
- **Skill 加载**：`novel-skills/` 及 `engine.rs` 中 system prompt 构建
- **Agent 级 Skill 文案**：`skills/`（运行时加载）
- **开发工作流 Skill**：`.claude/skills/`（仅 Cursor/Claude 开发用，不进 system prompt）

按范围核对：

1. **主路径** — IPC：`invoke` ↔ `EngineCommand` ↔ `engine_loop.rs`；`Event` ↔ 前端 `listen`。作品切换：`list_works` / `create_work` / `open_work` → `SwitchProjectAndCreateSession` → `config.write().set_active_project` → 新 `db_path` → `session-resumed`；`create_session()` 无 path 参数。Turn：`handle_message_with_events` → compaction（含 DB sync）→ inner loop → `drain_subagent_jobs`。API：`get/set_api_config` → 全局 json；`session_llm::build_chat_client`（`resolve_agent_api_key` env > json + `from_api_key_or_env`）；无 client 时 offline。
2. **失败传播** — 单队列串行；空消息、`drain_in_progress`（子 Agent drain）、未答 `AskUserQuestion`、嵌套 fork → `Validation`/`AgentBusy`；`LlmError`、DB、`NeedsUserInput`、`TemplatesNotFound` 等向上传播。锚点：`turn_loop.rs`、`engine.rs`、`engine_loop.rs`。
3. **不变量** — Engine 单队列；Hook 串行 drain；Read-before-write；Compaction：**先** `archive_session_messages` **再** `replace_session_messages`；system **AGENTS/Workspace** metadata 冻结，compact 时 `refresh_system_dynamic_sections`（Index/Memory/Progress/**Skills 摘要**）；Skill 全文 + 摘要经 **`[上下文刷新]` user `(0,1)`**；建会话即 persist `(0,0)` system；`invoked_skill_ids` 存 `metadata_json`；UI hydrate 走 `get_session_transcript`；Fork 仅从 `messages[0]`；scaffold 仅读 `templates/`。
4. **安全边界** — 文件工具：`resolve_path` + `validate_write_root`；作品名 `validate_work_name`、`ensure_work_under_works`；API Key 掩码、不进 log/emit；DB 参数化、guard 不跨 `.await`；`AppConfig` 为 `Arc<RwLock<_>>`，切换作品时 IPC 不读 stale path。
5. **交付物** — 缺测试本步补写：**改 crates → 步骤 4（A）补 `#[test]` / integration**；**改 `ui/` → 步骤 6（C）补 Vitest**；**改 Tauri 壳 → 步骤 5（B）** 确认能 `ci-tauri`；改 scaffold 须同步 `templates/**/*.md`；文档留步骤 9。
6. **Tracing** — 步骤 1.5：`info` 是否克制；失败路径有 `warn`/`error`；深度细节在 `debug`。

输出 3–6 条：`[结论] — 依据：<路径:行号>`（含 Tracing 结论时注明埋点位置或「不适用」）。

## 3. Rust 审查

对本次 diff 逐项检查，**每项须有证据或明确排除**，不是空打勾。步骤 2 负责**项目链路与安全边界**；本节负责**通用 Rust 编码质量与安全**。发现问题**先修再进入步骤 4–7**；修完则**回到步骤 1**。

### 3.1 编码规范（反模式）

| 反模式 | 禁止 / 警惕 | 推荐 |
|--------|-------------|------|
| **懒人式错误处理** | 生产路径 `unwrap()`/`expect()`、忽略 `Result`、深层 `format!` 丢上下文 | 库 crate 用 `thiserror`（`AgentError`、`StateError` 等）；DB/IO/JSON 用 `?`；Tauri 边界再 `map_err` 为 `String` |
| **忽视标准库 Trait** | 数据类型缺 `Debug`/`Clone`/`PartialEq`；自造转换 helper | 纯数据 `#[derive(Debug, Clone, PartialEq)]`；IPC DTO 加 serde；用 `From`/`TryFrom`/`Display` |
| **滥用 `clone()`** | 遇所有权问题即 `.clone()` 通过编译 | 优先借用；共享状态用 `Arc::clone`（`AppConfig`、`AbortController`）；热路径 clone 须有理由 |
| **模式匹配不足** | 枚举/`Option`/`Result` 用手动 if-else 解构 | `match` 穷尽（`EngineCommand`、`TerminalReason` 等）；`if let`/`matches!` |
| **通配符导入** | `use module::*`（测试 `super::*` 除外） | 显式导入；长名用别名 |

**本仓库额外警惕：**

- **DeepSeek 工具参数必须 snake_case**：DeepSeek API 的函数调用参数名遵循 snake_case（官方文档示例均为 `skill_id`、`user_email`）。所有 `tool.input_schema()` 的 `properties` 键名和 `required` 数组中字段名必须用 snake_case；`require_str` 调用的 key 必须与 schema 一致。**禁止新增 camelCase 参数名**（如 `skillId`、`chapterPath`）。
- **流式 tool call 参数累积**：`client.rs::try_emit_ready_tool` 在参数 JSON 完整前不得发射——`parse_tool_arguments("")` 返回 `Ok({})`，但空对象 `{}` 表示参数尚未到达（DeepSeek 的 `function.arguments` 首 chunk 为 `""`）。**必须在解析成功后检查对象非空**，否则后续 `require_str` 取不到任何字段。
- **JSON 边界**：`content_json`、`metadata_json` 解析失败须明确报错，勿 silent default。
- **测试与生产路径**：`repo_templates_dir()`、`TempDir` 仅 `#[cfg(test)]`；生产走 `resolve_agent_root()`。

### 3.2 安全审查

**原则：`unsafe` 与生产路径 `unwrap()` 禁止新增**；各 lib crate 根 `#![deny(clippy::unwrap_used)]` + `#![cfg_attr(test, allow(...))]`，CI / `ci-rust-gate.sh` 另加 `-D clippy::unwrap_used`。`.expect()` 仍仅限编译期不变量（字面量 Regex、`include_str!` 等）；DB/IO/网络/JSON 用 `?`。路径/API 策略见步骤 2，此处不重复。

| 类别 | 检查要点 |
|------|----------|
| `unsafe` | 原则上禁止新增；确有必要须 `// SAFETY:` 说明不变量 |
| `unwrap` | **仅** `#[cfg(test)]`、crate 内 `tests/`、`tests/integration/` 允许；生产 lib 一律禁止（独立 test binary 文件顶行 `#![allow(clippy::unwrap_used)]`） |
| `expect` | 生产业务路径禁止；编译期不变量可局部 `#allow`；无 `todo!()`/`unimplemented!()` |
| 死锁 / 锁与 await | 持锁不 `.await`；`AppConfig` 用 `tokio::sync::RwLock`；`engine_loop` 内写 config 后尽快释放锁再跑 turn |
| `RefCell` / 内部可变性 | 禁止嵌套 `borrow_mut`（运行时 panic）；`borrow` 期间勿调用可能再 borrow 同 cell 的回调；并发场景用 `Mutex`/`RwLock` 而非 `RefCell` |
| `Arc`/`Rc` 循环引用 | 双向强引用须以 `Weak` 打断；`spawn_event_forwarder` 等闭包勿与 `AppHandle`/engine 形成无法 drop 的环；长生命周期 listener 须能随 session/turn 结束释放 |
| 异步 task | `engine_loop`、`spawn_event_forwarder` 等须可取消、可 drop；避免 detached task 长期持有 engine |
| 阻塞 I/O | async 内同步文件 I/O / rusqlite 须评估 `spawn_blocking`，或限定在 engine 单队列内 |
| 敏感信息 | API Key 脱敏；不进 tracing/log/`emit`/对外 `Err`；只写全局 `api_config.json`，勿写 per-work DB |
| Tracing | 见 1.5：`info` 精简、失败必 log、禁止 `println!` |
| 数据库 | 参数化 SQL（`params!`）；连接 guard 不跨 `.await`；`replace_session_messages` 等批量写用事务 |

### 3.3 架构相关审查

触及下列模块时，除 3.1–3.2 外还须核对：

| 模块 | 要点 |
|------|------|
| `engine_loop.rs` | 单队列串行；`SwitchProjectAndCreateSession` 后旧 engine drop；`reply.send` 不 panic |
| `AppConfig` + IPC | 读 `config.read().await`，切换 `write()`；持锁期间不调用长耗时 turn |
| `turn_loop.rs` | `build_chat_client` / per-turn override 重建；`init_llm` 惰性；compaction 后 DB sync 与内存 messages 一致；续跑 `continue_turn_loop` 恢复 `inner_turn`；关键路径有 tracing（步骤 1.5） |
| `novel-logging` / `engine_loop.rs` | `init_logging`；IPC `info!`；会话 `agent.jsonl` 与 stderr 分工见 `novel-logging.md` |
| `scaffold` / `novel-state` | `TemplatesNotFound` 勿静默；`metadata_json` 与 message 写入同一 session |

### 3.4 审查输出

汇总一行：

```text
反模式: 通过（或 N 处已修） / unsafe: N / unwrap: 允许 N·需修 N / 并发·I/O·边界: 通过（或：<具体风险>）
```

任一项「需修」则**不得进入步骤 4–7**，修完后从步骤 1 重跑。

## 4. 后端验证（范围 A）

**何时跑：** 步骤 0 勾选 **A**。**未勾选则整节跳过**（汇报写「后端：未触达」）。

**顺序（与 CI 一致，勿省略）：**

```bash
bash scripts/ci-rust-static.sh   # cargo fmt --check + cargo check --workspace
bash scripts/ci-clippy.sh        # cargo clippy --workspace --all-targets …
bash scripts/ci-rust-test.sh     # cargo nextest run --workspace --profile ci
```

**硬性要求：** check / clippy **0 warning**（`-D warnings -D clippy::unwrap_used`）；nextest **非** `cargo test`，须 **0 failed**、**0 SLOW / TIMEOUT**。

改 turn / tool / state / compaction 等时尽可能覆盖主路径与失败传播；integration 放对应 crate 的 `tests/`。无法补测时在汇报说明。

<details><summary>慢测与网络测（遇 SLOW/TIMEOUT 时查阅）</summary>

- **SLOW [>60s]**：查未 mock 的网络、同步 I/O 阻塞、DashMap/RwLock 死锁
- **TIMEOUT**：必须修复；`cargo nextest run --test <name>` 单独复现
- Live API：需 `DEEPSEEK_API_KEY`；`cargo nextest run --run-ignored all`

</details>

## 5. Tauri 壳验证（范围 B）

**何时跑：** 步骤 0 勾选 **B**。**未勾选则跳过**（仅改 `crates/` 且未动 `src-tauri` / Tauri IPC 时通常不跑）。

```bash
bash scripts/ci-tauri.sh
```

含 Linux 依赖检查（本机可忽略失败时看脚本）、`ci-tauri-check`（`cargo check -p novel-agent`）、`ui` 构建产物与 **Tauri build**。只改 `tauri.conf.json` / 图标 / capabilities 时仍跑本条即可，**不必**跑步骤 4 的 nextest，除非步骤 0 也勾了 A。

## 6. 前端验证（范围 C）

**何时跑：** 步骤 0 勾选 **C**。**未勾选则跳过**。

```bash
bash scripts/ci-frontend.sh
```

等价 `ci-check-node.sh` + `ui` 下 `npm ci` / `npm test` / `npm run build`（以脚本为准）。核对 `invoke` / 事件名与 Rust 侧一致；`AppStatus` 含 `activeWorkName`/`projectRoot`。若改 IPC 契约，通常 **C + B**（及可能 **A**），并**回到步骤 1**。

**硬性要求：** **0 failed**；无 ERROR / DEPRECATED / vitest·esbuild 警告。

**测试补写：** 单元测 `ui/src/**/*.test.ts(x)`；fixture `ui/src/test/`。改 transcript / 聊天 UI 时尽可能覆盖流式 tool、ReAct、AskUserQuestion、Write 批准、Fork、INTERRUPT、HYDRATE 等。仅样式可标「无新增场景」，仍须 `npm test` 全绿。

## 7. 依赖安全 audit（范围 D，可选）

**何时跑：** 步骤 0 勾选 **D** 或发 PR 前与 `deps-audit` workflow 对齐。

```bash
bash scripts/ci-security-audit.sh
```

本地 GitHub 访问失败时可 `SKIP_SECURITY_AUDIT=1`（与 `ci-windows.ps1` 一致）；**发 PR 仍以 CI 为准**。新增 ignore 须写入 `.cargo/audit.toml` 并注明 Tauri 升级复查点。

## 8. Agent 级 Skill（`skills/`）

涉及运行时 `skills/` 时（验证通过后再改文案）：

- [ ] `SKILL.md` frontmatter + body
- [ ] `references/*.md` 与 body 链接
- [ ] 脚手架**不**创建 `{work}/skills/`；步骤 9 更新 `docs/crates/novel-skills.md`

`.claude/skills/` 为开发工作流，与运行时 `skills/` 分离。

## 9. 文档同步

**最后执行**——描述已通过 check/test/build 的行为。

| 改动范围 | 更新目标 |
|----------|----------|
| crate API/行为 | `docs/crates/<crate>.md` |
| 路径 / API json / AppConfig | `docs/crates/novel-config.md` |
| 作品 IPC / AppStatus | `docs/crates/novel-server.md`、`FRAMEWORK.md` |
| scaffold / templates / memory | `docs/crates/novel-knowledge.md` |
| compaction / metadata / DB sync | `docs/crates/novel-state.md`、`novel-compaction.md` |
| tracing / 审计日志 | `docs/crates/novel-logging.md` |
| Skill 规则 | `skills/<id>/` + `docs/crates/novel-skills.md` |
| 能力概览 | `README.md`、`docs/README.md` |

确认：无 `NOVEL_API_KEY`、`create_session(path)` 等过时描述。

## 10. 汇报模板

```markdown
## 收尾结果

- [x] 代码清理：<…>
- [x] Tracing 埋点：<… / 不适用>
- [x] 链路走查：<摘要>
- [x] Rust 审查：<3.4 汇总行>
- [x] 验证范围（步骤 0）：A / B / C / D — <勾选说明>
- [x] 后端（A）：<未触达跳过 / ci-rust-static + ci-clippy + ci-rust-test 通过>
- [x] Tauri 壳（B）：<未触达跳过 / ci-tauri 通过>
- [x] 前端（C）：<未触达跳过 / ci-frontend 通过>
- [x] audit（D）：<未跑 / 通过 / SKIP_SECURITY_AUDIT>
- [x] Skill / templates：<无变更 / 已更新>
- [x] 文档已更新：<文件列表>
```
