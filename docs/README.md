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
| [prompt/](../prompt/) | System / 子 Agent 提示词（`include_str!` 嵌入）；含 `agents/plan-auditor.md`、`knowledge-auditor.md`、`chapter-craft-analyzer.md`、`general_purpose.md`、`compaction-summary-trailing.md` |
| [skills/](../skills/) | Workflow + 流派 Skill（Agent 级；`works/{名}/skills/` 可覆盖同 id） |
| [templates/](../templates/) | 新建作品脚手架 Markdown（运行时读盘，必填） |
| [works/](../works/) | 用户作品实例（gitignore） |

## Skill 系统概要

Skill 文件夹格式：`skills/<id>/SKILL.md` + 可选 `references/`。

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
| 细纲后 **PlanAuditor**；正文后 **2 项 Subagent** 并行 Fork | `prompt/system.md` §1.2、§3.2 |
| 权限四模式 | `prompt/system.md` §2.1 |

引擎仅 enforce sandbox、Plan 路径、嵌套 fork 等；写后审计顺序由 prompt 约束，非 Rust 硬编码。

## 前端 UI 概要

| 功能 | 说明 |
|------|------|
| StatusBar 作品 | `list_works` 下拉 + 新建作品；切换作品会 **新建 session**（不自动恢复上次会话） |
| StatusBar 会话 | `list_sessions` 下拉 + `resume_session` 切换；`+` → `create_session`；标签显示 **对话轮数** + **最后 LLM 活跃时间**；流式中（`isStreaming`）禁用切换 |
| StatusBar Todo | **按钮常驻**（StatusBar 最左）；`TodoWrite` → DB → **`session-todos-updated`** → `useAppStatus.todos`（turn 进行中即时，不经 `get_app_status`）；下拉按 **进行中 / 未进行 / 已完成** 分组（已完成划掉）；**仍有未完成项**时列表可见；**全部完成或空列表**为「暂无待办事项」；`update_session_todo({ sessionId, todoId, status })` 点击循环状态并 emit 同事件；未完成数 **0→>0** 自动展开、**>0→0** 自动收起 |
| StatusBar Token | 会话累计三分类 + 当前上下文；**`session-tokens-updated` 事件驱动**（主/SubAgent 每次 LLM 调用后推送）；初始与切 session 经 `get_app_status`；30s 轮询与 `turn-complete` / `permission-mode-changed` refresh 兜底 turn 标志等非 token 字段 |
| 设置 · 会话列表 | 同 `list_sessions`；元数据含 **对话 N 轮 · API M 次** |
| 权限 / 模型 | ChatPanel 底栏：normal / plan / auto / unattended；flash/pro；**turn 进行中禁用** |
| **聊天区布局** | Agent/用户/Subagent 全宽 `message`；问答全宽卡片；普通工具 `message-tool` + `ToolUseCard`；`word-break` 边界换行 |
| **Turn 折叠 + Sticky** | 最后一轮 `transcript-turn-anchor` 最小高度 = 视口（折叠较早轮）；用户气泡紧挨上一轮、流式内容向下生长；近底跟随 + 发送 `pinAndScrollToBottom`；用户提问滚出上方时 **sticky-prompt-header** |
| **Turn 内存与滚动** | `loadPolicy` 三档（TAIL **6** / VIEW **6** / MAX **18**）；`planMemoryReconcile` 统一预取与淘汰；贴底欠填向上预取；`useTranscriptLoader` 编排 `EVICT_TURNS`（仅 FSM，DB 保留）；`ScrollViewport` 近底跟随（128px）+ `onBottomAnchorChange` 防抖收缩 |
| **Transcript FSM** | `ui/src/transcript/`：`dispatchTranscriptEvent` 管理 Turn / LlmSegment / openSegment；Tauri 事件经 `mapEvents` 适配；`liveTail` 处理流式 orphan 尾轮 |
| **SegmentGroup 渲染** | `TranscriptView` 唯一入口；`segmentRender.tsx` 成组 Agent + Tool；主聊天、AskUserQuestion、SubAgent overlay 共用 |
| **SubAgentForkCard** | tool 路径：段内 `ForkSubAgent`；hook 路径：`HookForkCards` 列在滚动区底部。与 Agent 同构（`Subagent · {类型}`）；**进入** → `SubAgentOverlay`（`mode=fork`，含 `forkRuns` 与 approve/deny） |
| 流式 Tool | `ToolUseCard`（虚线内卡）显示 pending / running / done；`ForkSubAgent` 走 `SubAgentForkCard` 而非 `ToolUseCard` |
| AskUserQuestion | 全宽卡片；插在 `pauseAfterSegmentId` 段 tools 之后；事件 payload `allowMultiple` / `allowCustom`（camelCase） |
| **CompactionDivider / ContextRefreshBubble** | archive 区分隔线（含 epoch + 保留 turn 范围，来自 layout `retainedMinTurn`/`retainedMaxTurn`）；`[上下文刷新]` 单气泡（非合并 system+ReAct）：折叠优先审计 preview + Skill 标签；展开「审计状态」块 + 会话摘要 |
| 压缩进度 | `compaction-progress` → **CompactionBanner**（`dialog-viewport` 顶部） |

**会话术语：** 见 [novel-state §1.4](crates/novel-state.md#14-sessionsummary)（`total_turns` vs `api_call_count` vs `context_tokens` vs `last_active_at`）。

## Subagent 双轨模型

| 模式 | 触发 | 典型场景 |
|------|------|----------|
| Workflow Skill | InvokeSkill | 策划、写章、改稿、写后收尾 |
| 计划审计 Subagent | ForkSubAgent（细纲 + 追踪文件更新后） | PlanAuditor（大纲对齐、伏笔密度、因果闭合等） |
| 写后检查 Subagent | ForkSubAgent（正文写后同批 2 项） | KnowledgeAuditor（执行忠实度）、ChapterCraftAnalyzer（文笔 + 设定一致性） |
| GeneralPurpose | ForkSubAgent，task = 完整 prompt | 只读自定义调研/分析（完整报告；正典写盘交主 Agent） |

主 LLM 仅见工具路径的一条 `[子 Agent 完成: …]` 摘要；完整 transcript 在 `fork_messages` + overlay。Hook 路径（KnowledgeAuditor）**不 inject** 主会话。

## 前后端 Tool 事件契约

```
start → input_delta → input_complete → (pending | running) → progress/result
  result 阶段仅含 toolCallId + content（无 toolName）→ useAgent → Transcript FSM
assistant-segment-complete → 主聊天或 fork overlay 分段 finalize
ask-user-question → questions[] 含 allowMultiple / allowCustom（camelCase）
session-todos-updated → useAppStatus 局部 patch todos（TodoWrite / update_session_todo；turn 进行中即可）
turn-complete → onTurnComplete 单次 get_app_status（turn 标志等；todos 非主路径）
session-tokens-updated → useAppStatus 局部 patch token 四字段（主/SubAgent LLM 后推送）
session-resumed → useAgent 清 streaming；status.sessionId 更新 / compaction done → useTranscriptLoader.resetAndBootstrap；turn-complete → reloadActiveTail（get_session_message_turns 尾轮）；懒加载按时间轴相邻 idle 窗口预取（`turnLoadPlan`），跨 compact 并行分段 IPC（archive / active）；`planMemoryReconcile`（TAIL 6 / VIEW 6 / MAX 18，视口感知溢出淘汰）；贴底欠填 `TAIL_CONTENT_UNDERFLOW_PX` 向上预取；贴底区（`BOTTOM_ANCHOR_THRESHOLD_PX`）稳定后防抖收缩至 TAIL 6 轮；`ScrollViewport` 近底区内容增长时自动置底
```

详见 [novel-server §1.9](crates/novel-server.md#19-前端事件eventsrs) 与 [novel-core §1.10](crates/novel-core.md#110-流式-tool-调度streamingtooldispatch)。

## CI / CD

**Node.js 24**（`ui/.nvmrc`）— 本地与 GitHub 均由 `scripts/ci-check-node.sh` 在 `npm` 前校验；GHA 另设 `FORCE_JAVASCRIPT_ACTIONS_TO_NODE24` 与 `setup-node` + `node-version-file: ui/.nvmrc`。

### 本地

```powershell
.\scripts\ci-windows.ps1   # Windows 本地全量（GHA rust-windows + 本地 audit，见下表）
.\scripts\ci-local.ps1     # 自动选 Windows / Linux / macOS gate
```

```bash
bash scripts/ci-pr-gate.sh      # Linux / macOS 全量
bash scripts/ci-rust-gate.sh    # 仅 Rust + Tauri
bash scripts/ci-rust-test.sh    # 仅 nextest
```

Windows 请用 **Git Bash**（勿用 WSL `bash`）。`cargo audit` 需访问 GitHub；可 `SKIP_SECURITY_AUDIT=1` 跳过。

完整脚本表见 **[scripts/README.md](../scripts/README.md)**。

### GitHub Actions

一次 PR 的**门禁覆盖**（不必每个 runner 重复三项）：Ubuntu `frontend`（Vitest + build）+ 三平台 `ci-rust-gate.sh` + Ubuntu `security-audit`（`cargo audit`）。`rust-windows` 额外跑前端以对齐本地 `ci-windows.ps1`；**macOS rust job 不跑 Vitest 不是漏测**（前端已由 Ubuntu `frontend` 挡住）。

| Job | Runner | 脚本 |
|-----|--------|------|
| Frontend | ubuntu | `ci-frontend.sh` |
| Rust (Ubuntu) | ubuntu | `ci-rust-gate.sh` |
| Rust (Windows) | windows | `ci-frontend.sh` + `ci-rust-gate.sh` |
| Rust (macOS) | macos | `ci-rust-gate.sh` |
| Security audit | ubuntu | `ci-security-audit.sh`（仅此 job；Win/mac 不重复） |

Workflow：[ci.yml](../.github/workflows/ci.yml) · Release：[release.yml](../.github/workflows/release.yml)

## 清理作品会话库

删除 `works/**/.novel-agent/state.db*`（保留正文、知识库与 settings）：

```powershell
.\scripts\reset-work-databases.ps1   # Windows
./scripts/reset-work-databases.sh  # Git Bash / CI
```

详见 [README §清理作品会话库](../README.md#清理作品会话库)。
