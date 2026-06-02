# Novel Agent — 文档索引

> 项目入口：[README.md](../README.md) · 架构总览：[FRAMEWORK.md](../FRAMEWORK.md)

---

## Crate 专题

| Crate | 文档 |
|-------|------|
| novel-core | [novel-core.md](crates/novel-core.md) |
| novel-deepseek | [novel-deepseek.md](crates/novel-deepseek.md) |
| novel-tools | [novel-tools.md](crates/novel-tools.md) |
| novel-knowledge | [novel-knowledge.md](crates/novel-knowledge.md) |
| novel-state | [novel-state.md](crates/novel-state.md) |
| novel-compaction | [novel-compaction.md](crates/novel-compaction.md) |
| novel-config | [novel-config.md](crates/novel-config.md) |
| novel-skills | [novel-skills.md](crates/novel-skills.md) |
| novel-logging | [novel-logging.md](crates/novel-logging.md) |
| novel-server | [novel-server.md](crates/novel-server.md) |

## 阅读路径

| 目标 | 建议顺序 |
|------|----------|
| 首次上手 | [README](../README.md) → [novel-config](crates/novel-config.md)（路径布局）→ [novel-server](crates/novel-server.md) |
| 改 UI / IPC | [novel-server](crates/novel-server.md) · [FRAMEWORK §2.5](../FRAMEWORK.md#25-前端状态与-ipc) |
| 改 Agent 循环 / 流式 Tool / Subagent | [novel-core](crates/novel-core.md) §1.2 · [novel-tools](crates/novel-tools.md) · [FRAMEWORK §2.3](../FRAMEWORK.md#23-fork-子-agent) |
| 改知识库 / 脚手架 | [novel-knowledge](crates/novel-knowledge.md) · `templates/` 目录 |
| 改 Skill / Prompt | [novel-skills](crates/novel-skills.md) · [prompt/system.md](../prompt/system.md) |
| 改持久化 / 会话列表 / Token | [novel-state](crates/novel-state.md)（`accumulate_session_tokens` / `context_tokens` / `total_turns` / `api_call_count` / `last_active_at`） |

## 运行时资产（非 crate 代码）

| 路径 | 说明 |
|------|------|
| [prompt/](../prompt/) | System / 子 Agent 提示词（`include_str!` 嵌入）；含 `agents/general_purpose.md`、`compaction-summary-trailing.md`（压缩摘要 trailing user） |
| [skills/](../skills/) | Workflow + 流派 Skill（Agent 级；`works/{名}/skills/` 可覆盖同 id） |
| [templates/](../templates/) | 新建作品脚手架 Markdown（运行时读盘，必填） |
| [works/](../works/) | 用户作品实例（gitignore） |

## Skill 系统概要

Claude Code 文件夹格式：`skills/<id>/SKILL.md` + 可选 `references/`。

- **摘要** → session 创建时注入 system prompt  
- **正文** → InvokeSkill 按需加载  
- **References** → Read 渐进打开  
- **作品覆盖** → `load_skills_merged(works/{名}/skills, agent/skills)`  

压缩后 Skill 全文与摘要合并为 `[上下文刷新]` user；Memory/INDEX/Progress 经 system 动态段刷新。详见 [novel-compaction.md](crates/novel-compaction.md)。

## Agent 行为概要（prompt 层）

| 主题 | 位置 |
|------|------|
| 创作顺序（大纲→细纲→正文） | `prompt/system.md` §1 |
| **读盘经济**（Grep 优先、分段 Read） | `prompt/system.md` §2.3、§5.1 |
| 写后 **2 项 Subagent** 并行 Fork | `prompt/system.md` §1.2、§3.2 |
| 权限四模式 | `prompt/system.md` §2.1 |

引擎仅 enforce sandbox、Plan 路径、嵌套 fork 等；写后审计顺序由 prompt 约束，非 Rust 硬编码。

## 前端 UI 概要

| 功能 | 说明 |
|------|------|
| StatusBar 作品 | `list_works` 下拉 + 新建作品；切换作品会 **新建 session**（不自动恢复上次会话） |
| StatusBar 会话 | `list_sessions` 下拉 + `resume_session` 切换；`+` → `create_session`；标签显示 **对话轮数** + **最后 LLM 活跃时间** |
| StatusBar Todo | TodoWrite 持久化项的下拉列表（非独立 TodoPanel）；全部 completed/cancelled 后自动隐藏 |
| StatusBar Token | 本轮三分类 + 会话累计 + 当前上下文；`get_app_status` 5s 轮询 + `turn-complete` 刷新 |
| 设置 · 会话列表 | 同 `list_sessions`；元数据含 **对话 N 轮 · API M 次** |
| 权限 / 模型 | normal / plan / auto / unattended；flash/pro 切换；**turn 进行中禁用** |
| **Transcript FSM** | `ui/src/transcript/`：`dispatchTranscriptEvent` 管理 Turn / LlmSegment / openSegment；Tauri 事件经 `mapEvents` 适配 |
| **SegmentGroup 渲染** | `TranscriptView` 唯一入口；`segmentRender.tsx` 成组 Agent + Tool 气泡；主聊天、AskUserQuestion、SubAgent overlay 共用 |
| **SubAgentForkCard** | ForkSubAgent 工具卡与 Hook 卡；**进入** 打开 `SubAgentOverlay`（`TranscriptView mode=fork`） |
| 流式 Tool | `ToolCall.status=streaming-args` 流式累积参数；`ToolUseCard` 显示 pending / running / done |
| AskUserQuestion | 问答面板插在 `pauseAfterSegmentId` 对应段的全部 tools 之后 |
| **CompactionDivider / ContextRefreshBubble** | archive 区分隔线；`[上下文刷新]` 单气泡（Skill + 摘要两节） |
| 压缩进度 | `compaction-progress` → **CompactionBanner**（ChatPanel viewport 顶部，已接入） |

**会话术语：** 见 [novel-state §1.4](crates/novel-state.md#14-sessionsummary)（`total_turns` vs `api_call_count` vs `context_tokens` vs `last_active_at`）。

## Subagent 双轨模型

| 模式 | 触发 | 典型场景 |
|------|------|----------|
| Workflow Skill | InvokeSkill | 策划、写章、改稿、写后收尾 |
| 检查 Subagent | ForkSubAgent（同批 2 项写后必做） | 知识库审计（KnowledgeAuditor）、章节技艺分析（ChapterCraftAnalyzer） |
| GeneralPurpose | ForkSubAgent，task = 完整 prompt | 一次性自定义任务 |

主 LLM 仅见工具路径的一条 `[子 Agent 完成: …]` 摘要；完整 transcript 在 `fork_messages` + overlay。Hook 路径（KnowledgeAuditor）**不 inject** 主会话。

## 前后端 Tool 事件契约

```
start → input_delta → input_complete → (pending | running) → progress/result
assistant-segment-complete → 主聊天或 fork overlay 分段 finalize
turn-complete → StatusBar token refresh；若 pending 工具/问答则前端不 hydrate
session-resumed / turn-complete → useAgent hydrate（get_session_transcript）
```

详见 [novel-server §1.9](crates/novel-server.md#19-前端事件eventsrs) 与 [novel-core §1.10](crates/novel-core.md#110-流式-tool-调度streamingtooldispatch)。

## CI / CD

### 本地（与 GitHub 门禁一致）

```powershell
.\scripts\ci-windows.ps1   # Windows：与 GitHub rust-windows job 相同（推荐）
.\scripts\ci-local.ps1     # Windows → ci-windows-gate；Linux/macOS → ci-pr-gate
.\scripts\verify_all.ps1   # ci-local.ps1 别名
.\scripts\run_tests.ps1    # 仅后端 nextest --profile ci
```

Windows 上若直接 `bash scripts/ci-tauri-check.sh`，请用 **Git Bash**（`Program Files\Git\bin\bash.exe`），不要用 WSL 的 `C:\Windows\System32\bash.exe`——后者 PATH 里没有 Windows 版 `cargo`，会报 `cargo: command not found`。

`ci-windows.ps1` 最后一步 `cargo audit` 需访问 GitHub 拉取 RustSec 漏洞库；网络不通时会失败（与 Tauri/Rust 编译无关）。可修复网络后重跑，或本地临时跳过：`$env:SKIP_SECURITY_AUDIT='1'; .\scripts\ci-windows.ps1`。若曾成功拉取过，脚本会自动尝试 `--no-fetch` 使用 `~/.cargo/advisory-db` 缓存。

| 脚本 | 说明 |
|------|------|
| **`ci-rust-gate.sh`** | **三平台 Rust 统一门禁**：fmt + check + clippy + nextest + Tauri（`ci-rust-gate`） |
| `ci-gate-core.sh` | 本地/Linux 全量：frontend + `ci-rust-gate` + audit |
| `ci-windows-gate.sh` | Windows 全量：frontend + `ci-rust-gate` + audit（= GHA `rust-windows`） |
| `ci-pr-gate.sh` | 非 Windows 本地入口 → `ci-gate-core` |
| `ci-rust-test.sh` | nextest（`concurrent_writes` 压测 + 全 workspace；含 `ci-nextest-env.sh`） |
| `ci-linux-tauri-deps.sh` | 仅 GHA Ubuntu：WebKit/GTK apt（由 `ci-rust-gate` 调用） |
| `ci-security-audit.sh` | `cargo audit`（Ubuntu 独立 job；Win/mac 在 `rust-*` job 内） |

### GitHub Actions（Rust 三平台对齐）

| Job | 平台 | Rust 步骤 | 额外 |
|-----|------|-----------|------|
| **rust** | Ubuntu | `ci-rust-gate.sh` | `frontend` 独立 job；`security-audit` 独立 job |
| **rust-windows** | Windows | `ci-rust-gate.sh` | 同 job 内 `ci-frontend` + `ci-security-audit` |
| **rust-macos** | macOS | `ci-rust-gate.sh` | 同 job 内 `ci-security-audit` |

三平台 `ci-rust-gate.sh` 相同：rustfmt → check → clippy → nextest（`NEXTEST_TEST_THREADS` + `concurrent_writes`×10）→ Tauri check + **cargo build**。

| Workflow | 触发 | 说明 |
|----------|------|------|
| [`.github/workflows/ci.yml`](../.github/workflows/ci.yml) | push/PR | 上表三 job + frontend + audit |
| [`.github/workflows/release.yml`](../.github/workflows/release.yml) | tag `v*` | gate 含 `ci-rust-gate.sh`，再三平台打包 |

本地：`.\scripts\ci-windows.ps1`（Win）或 `bash scripts/ci-pr-gate.sh`（Linux/mac）。仅测 Rust：`bash scripts/ci-rust-gate.sh`（Git Bash，勿用 WSL）。`.\scripts\run_tests.ps1` = `ci-rust-test.sh`。

## 清理作品会话库

删除 `works/**/.novel-agent/state.db*`（保留正文、知识库与 settings）：

```powershell
.\scripts\reset-work-databases.ps1   # Windows
./scripts/reset-work-databases.sh  # Git Bash / CI
```

详见 [README §清理作品会话库](../README.md#清理作品会话库)。
