---
name: rust-post-change
description: >-
  Rust 后端（crates/）修改后收尾：refactor-cleanup、Tracing 埋点、链路走查、Rust 审查、
  然后跑 ci-rust-gate（fmt + clippy + nextest + Tauri 壳）。
  仅改 crates/ 且未触达 Tauri/UI 时使用本 skill；跨层改动见 post-change-checklist。
---

# Rust 后端修改后收尾

在 **novel_agent** 内修改 `crates/**` 或 `tests/integration/**` 后执行。
若同时改了 `ui/` 或 `src-tauri/`，改用 [`post-change-checklist`](../post-change-checklist/SKILL.md)。

**Rust 测试：全项目只用 `cargo nextest run`（及 `cargo llvm-cov nextest` 做覆盖率），禁止 `cargo test`。**

## 执行顺序（任一步改代码须从头重跑）

### 1. 代码清理（必做 · 读 [`refactor-cleanup`](../refactor-cleanup/SKILL.md)）

执行 [`refactor-cleanup`](../refactor-cleanup/SKILL.md) 全流程，聚焦 **Rust 后端层**（`crates/**`、`tests/integration/**`）：

- 删除已无调用方的函数、模块、配置项
- 收敛仅为旧行为服务的向后兼容逻辑、deprecated 别名
- 删除因重构闲置的 import、常量、测试桩
- 注释与文档：与代码不一致时改写到反映当前行为；严禁删注释掩盖不一致

### 1.5. Tracing 埋点

改业务逻辑须补可观测性，但 dev 终端要短：默认 `info` 只打 turn 边界与失败；其余用 `debug`。

| 级别 | 用途 |
|------|------|
| `info!` | `turn_start` / `turn_complete`（仅 `turn` + `reason`）；压缩起止 |
| `warn!` / `error!` | LLM/工具/DB 失败、校验拒绝、circuit breaker |
| `debug!` | IPC、LLM 请求、每笔 tool、落库、token、续跑/子 Agent |
| `trace!` | 高频轮询（`GetStatus`） |

**禁止** API Key、tool 全文进 log。里程碑写 `audit_log`。纯文档/重命名可标「Tracing：不适用」。

### 2. 链路走查

通读本次 diff，沿代码链路逐段核对。先判断改动类别：

- **Turn / 工具**：`novel-core/`、`novel-tools/`
- **状态 / 知识库**：`novel-state/`、`novel-knowledge/`（含 scaffold、templates）
- **配置 / 作品**：`novel-config/`（paths、agent_config）、`AppConfig`、`main.rs` setup
- **Compaction / Prompt**：`novel-compaction/`、`prompt/`
- **Skill 加载**：`novel-skills/` 及 `engine.rs` 中 system prompt 构建

按范围核对：

1. **主路径** — Turn：`handle_message_with_events` → compaction（含 DB sync）→ inner loop → `drain_subagent_jobs`。API：`get/set_api_config` → 全局 json；`session_llm::build_chat_client`。配置：`resolve_agent_root()`、`AppConfig` 初始化。
2. **失败传播** — 单队列串行；空消息、`drain_in_progress`、未答 `AskUserQuestion`、嵌套 fork → `Validation`/`AgentBusy`；`LlmError`、DB、`NeedsUserInput`、`TemplatesNotFound` 向上传播。
3. **不变量** — Engine 单队列；Hook 串行 drain；Read-before-write；Compaction 先 archive 再 replace；system 元数据冻结，compact 时 `refresh_system_dynamic_sections`；建会话即 persist system。
4. **安全边界** — 文件工具：`resolve_path` + `validate_write_root`；API Key 掩码、不进 log/emit；DB 参数化、guard 不跨 `.await`；`AppConfig` 为 `Arc<RwLock<_>>`。
5. **交付物** — 缺测试本步补写：改 crates → 补 `#[test]` / integration；改 scaffold 须同步 `templates/**/*.md`。
6. **Tracing** — 步骤 1.5：`info` 克制；失败路径有 `warn`/`error`；深度细节在 `debug`。

### 3. Rust 审查

#### 3.1 编码规范

| 反模式 | 禁止 / 警惕 | 推荐 |
|--------|-------------|------|
| 懒人式错误处理 | 生产路径 `unwrap()`/`expect()`、忽略 `Result` | 库 crate 用 `thiserror`；DB/IO/JSON 用 `?` |
| 忽视标准库 Trait | 数据类型缺 `Debug`/`Clone`/`PartialEq` | 纯数据 `#[derive(Debug, Clone, PartialEq)]` |
| 滥用 `clone()` | 遇所有权问题即 `.clone()` | 优先借用；共享状态用 `Arc::clone` |
| 模式匹配不足 | 枚举/`Option`/`Result` 用手动 if-else | `match` 穷尽；`if let`/`matches!` |
| 通配符导入 | `use module::*`（测试除外） | 显式导入 |

**本仓库额外警惕：**
- **DeepSeek 工具参数必须 snake_case**：所有 `tool.input_schema()` 的 `properties` 键名和 `required` 字段名必须用 snake_case。
- **流式 tool call 参数累积**：`parse_tool_arguments("")` 返回 `Ok({})`，必须在解析成功后检查对象非空。
- **JSON 边界**：`content_json`、`metadata_json` 解析失败须明确报错，勿 silent default。
- **测试与生产路径**：`repo_templates_dir()`、`TempDir` 仅 `#[cfg(test)]`。

#### 3.2 安全审查

| 类别 | 检查要点 |
|------|----------|
| `unsafe` | 原则上禁止新增；确有必要须 `// SAFETY:` 说明不变量 |
| `unwrap` | **仅** `#[cfg(test)]`、`tests/` 允许；生产 lib 禁止 |
| `expect` | 生产业务路径禁止；编译期不变量可局部 `#allow` |
| 死锁 / 锁与 await | 持锁不 `.await`；`AppConfig` 用 `tokio::sync::RwLock` |
| `RefCell` / 内部可变性 | 禁止嵌套 `borrow_mut`；并发场景用 `Mutex`/`RwLock` |
| `Arc`/`Rc` 循环引用 | 双向强引用须以 `Weak` 打断 |
| 异步 task | `engine_loop`、`spawn_event_forwarder` 须可取消、可 drop |
| 阻塞 I/O | async 内同步文件 I/O / rusqlite 须评估 `spawn_blocking` |
| 敏感信息 | API Key 脱敏；不进 tracing/log/emit/对外 `Err` |
| 数据库 | 参数化 SQL（`params!`）；连接 guard 不跨 `.await` |

#### 3.3 架构审查

触及下列模块时额外核对：

| 模块 | 要点 |
|------|------|
| `engine_loop.rs` | 单队列串行；`SwitchProjectAndCreateSession` 后旧 engine drop |
| `AppConfig` + IPC | 读 `config.read().await`，切换 `write()`；持锁期间不调用长耗时 turn |
| `turn_loop.rs` | `build_chat_client` / per-turn override；compaction 后 DB sync 一致 |
| `novel-logging` | `init_logging`；会话 `agent.jsonl` 与 stderr 分工 |

#### 3.4 审查输出

```text
反模式: 通过（或 N 处已修） / unsafe: N / unwrap: 允许 N·需修 N / 并发·I/O·边界: 通过（或：<具体风险>）
```

任一项「需修」则不得进入步骤 4，修完后从步骤 1 重跑。

### 4. 后端验证

```bash
bash scripts/ci-rust-static.sh   # cargo fmt --check + cargo check --workspace
bash scripts/ci-clippy.sh        # cargo clippy（-D warnings -D clippy::unwrap_used -D clippy::expect_used）
bash scripts/ci-rust-test.sh     # cargo nextest run --workspace --profile ci
bash scripts/ci-tauri.sh         # Tauri 壳 check + build
bash scripts/ci-lcov.sh && bash scripts/ci-crap.sh  # CRAP 门禁（threshold 20）
```

**硬性要求：** check / clippy **0 warning**（含 `unwrap_used` + `expect_used`）；nextest **0 failed**、**0 SLOW / TIMEOUT**；crap **none exceed CRAP threshold 20**。

### 5. 可选：IPC 冒烟

若改动触及 `engine_loop.rs`、`turn_loop.rs` 的 Fork/子 Agent 逻辑，见 [`smoke-post-change`](../smoke-post-change/SKILL.md)。

### 汇报模板

```markdown
## Rust 后端收尾

- [x] 代码清理：<…>
- [x] Tracing 埋点：<… / 不适用>
- [x] 链路走查：<摘要>
- [x] Rust 审查：<3.4 汇总行>
- [x] 后端验证：ci-rust-static / ci-clippy / ci-rust-test — 通过
- [x] CRAP（可选）：<通过 / 跳过>
- [x] 文档已更新：<文件列表>
```
