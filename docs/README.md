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
| 改持久化 | [novel-state](crates/novel-state.md) |

## 运行时资产（非 crate 代码）

| 路径 | 说明 |
|------|------|
| [prompt/](../prompt/) | System / 子 Agent 提示词（`include_str!` 嵌入）；含 `agents/general_purpose.md` |
| [skills/](../skills/) | Workflow + 流派 Skill（Agent 级；`works/{名}/skills/` 可覆盖同 id） |
| [templates/](../templates/) | 新建作品脚手架 Markdown（运行时读盘，必填） |
| [works/](../works/) | 用户作品实例（gitignore） |

## Skill 系统概要

Claude Code 文件夹格式：`skills/<id>/SKILL.md` + 可选 `references/`。

- **摘要** → session 创建时注入 system prompt  
- **正文** → InvokeSkill 按需加载  
- **References** → Read 渐进打开  
- **作品覆盖** → `load_skills_merged(works/{名}/skills, agent/skills)`  

压缩 refresh 重注入已 Invoke 的全文。详见 [novel-skills.md](crates/novel-skills.md)。

## Agent 行为概要（prompt 层）

| 主题 | 位置 |
|------|------|
| 创作顺序（大纲→细纲→正文） | `prompt/system.md` §1 |
| **读盘经济**（Grep 优先、分段 Read） | `prompt/system.md` §2.3、§5.1 |
| 写后 **5 项 Subagent** 并行 Fork | `prompt/system.md` §1.2、§3.2 |
| 权限四模式 | `prompt/system.md` §2.1 |

引擎仅 enforce sandbox、Plan 路径、嵌套 fork 等；写后审计顺序由 prompt 约束，非 Rust 硬编码。

## 前端 UI 概要

| 功能 | 说明 |
|------|------|
| StatusBar 作品 | `list_works` 下拉 + 新建作品 |
| StatusBar 会话 | 当前作品内 `create_session` / `resume_session` |
| StatusBar Token | 本轮三分类 + 会话累计；中断后 drain 估算 |
| 权限模式 | normal / plan / auto / unattended |
| 流式 Tool | `ToolUseCard`：pending 批准、running、结构化 input |
| AskUserQuestion | 问答面板；turn 暂停至 `answer_question` |
| Subagent | StatusBar chip + `SubAgentOverlay`（分段气泡，无 task 顶栏） |
| 压缩进度 | CompactionBanner |

## Subagent 双轨模型

| 模式 | 触发 | 典型场景 |
|------|------|----------|
| Workflow Skill | InvokeSkill | 策划、写章、改稿、写后收尾 |
| 检查 Subagent | ForkSubAgent（同批 5 项写后必做） | 一致性、日志遗漏、三 Analyzer |
| GeneralPurpose | ForkSubAgent，task = 完整 prompt | 一次性自定义任务 |

主 LLM 仅见工具路径的一条 `[子 Agent 完成: …]` 摘要；完整 transcript 在 `fork_messages` + overlay。Hook 路径（KnowledgeAuditor）**不 inject** 主会话。

## 前后端 Tool 事件契约

```
start → input_delta → input_complete → (pending | running) → progress/result
assistant-segment-complete → 主聊天或 fork overlay 分段 finalize
turn-complete → 若 pending 工具/问答则前端不 hydrate
```

详见 [novel-server §1.8](crates/novel-server.md#18-前端事件eventsrs) 与 [novel-core §1.10](crates/novel-core.md#110-流式-tool-调度streamingtooldispatch)。

## CI / CD（GitHub Actions）

| Workflow | 触发 | 说明 |
|----------|------|------|
| [`.github/workflows/ci.yml`](../.github/workflows/ci.yml) | push/PR → main | fmt + clippy + nextest + 前端 build/test；Ubuntu Tauri 编译冒烟；**无需 API Key** |
| [`.github/workflows/release.yml`](../.github/workflows/release.yml) | tag `v*` / 手动 | 质量门禁后三平台 Tauri 打包（需 `src-tauri/icons/icon.ico`） |
| [`.github/workflows/deps-audit.yml`](../.github/workflows/deps-audit.yml) | 每周 / 手动 | `cargo udeps`（不阻塞 PR） |

PR 仅跑 Ubuntu 全量 Rust 检查；push 到 main 额外跑 Windows/macOS nextest 矩阵。
