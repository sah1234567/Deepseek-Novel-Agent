# DeepSeek Novel Agent

面向长篇小说创作的 **AI 协作桌面应用**。作者与 Agent 多轮对话，在作品目录内完成策划、写章、改稿与设定维护。知识库、章节正文与跨会话记忆均以 Markdown 文件落盘，可用 Git 管理、人工审阅。

基于 Rust + Tauri + React 构建，对接 **DeepSeek V4 Pro / V4 Flash**（百万上下文、流式工具调用）。前端一键切换模型，无需重启。

**核心特点：** 创作流程按大纲→细纲→正文逐级推进，Workflow Skill 约束各阶段 SOP。每章写完后自动并行运行**知识库审计 + 章节质量分析**两个子 Agent，检查设定一致性、对话节奏、伏笔衔接等问题——子 Agent 独立上下文跑完，主 Agent 接收报告后按建议修复，再向作者宣告完成。详见 [FRAMEWORK.md](FRAMEWORK.md)。

---

## 能做什么

| 能力 | 说明 |
|------|------|
| **作品管理** | 多作品并存，各自独立的知识库、章节与创作历史 |
| **策划** | 大纲 → 细纲 → 人物卡 → 伏笔与因果链，逐级细化 |
| **写章** | 按细纲撰写正文，写后自动同步角色/场景/伏笔追踪表 |
| **改稿** | 影响分析 + 级联修改正文与关联设定 |
| **质量检查** | 写后自动审计：知识库遗漏、设定矛盾、对话节奏、情感轨迹 |
| **流派扩展** | 30+ 题材 Skill（仙侠、科幻、快穿等）按需加载 |
| **权限模式** | 常规 / 策划 / 自动 / 无人值守，控制写操作是否需确认 |

Agent 在作品目录 sandbox 内读写 `knowledge/`、`chapters/`、`memory/`；Workflow Skill 约束创作顺序，确保先大纲后正文、写后必审计。

---

## 目录概览

```
novel_agent/
├── works/{作品名}/          # 用户作品（知识库、章节、settings）
│   └── .novel-agent/state.db   # 该作品的 sessions / messages / todos（每作品独立）
├── skills/                  # Agent 级 Skill（作品可在 works/{名}/skills/ 覆盖同 id）
├── templates/               # 新建作品脚手架（必填）
├── prompt/                  # System 与子 Agent 提示词
└── .novel-agent/            # 全局 API 配置等
```

作品数据在 `works/` 下，与 Agent 代码分离。切换作品时前端同步切换会话库与文件树。审计日志在 `{作品}/.novel/logs/`。

### 清理作品会话库

需要清空对话历史时，可删除 `works/**/.novel-agent/state.db*`（不影响 `knowledge/`、`chapters/`、`settings.json` 与审计日志）：

```powershell
# Windows
.\scripts\reset-work-databases.ps1

# Git Bash / Linux / macOS
./scripts/reset-work-databases.sh
```

运行后重启应用，在 StatusBar 用 `+` 新建 session 即可。

---

## 快速开始

**前置：** [Rust](https://rustup.rs)、[Node.js](https://nodejs.org)、Tauri 系统依赖（Windows 需 WebView2）。Agent 根目录下须有 `templates/` 与 `skills/`。

```bash
cd novel_agent
cd ui && npm install && cd ..
```

**开发模式（推荐）：**

```bash
cargo tauri dev
```

自动编译 Rust、启动前端并打开桌面窗口。改 Rust 代码需重启命令；只改 `ui/` 则 Vite 热更新。

**构建安装包：**

```bash
cargo tauri build
```

**API Key（任选其一）：**

- 环境变量 `DEEPSEEK_API_KEY`（优先）
- 应用内 Settings 写入 `.novel-agent/api_config.json`
- 未配置时使用离线 mock（无真实 LLM）

可选环境变量：`DEEPSEEK_API_BASE`、`NOVEL_MODEL`、`NOVEL_COMPACTION_THRESHOLD` 等（见 [novel-config](docs/crates/novel-config.md)）。

**测试与 CI：**

```powershell
.\scripts\ci-windows.ps1  # Windows：与 GitHub rust-windows 相同（推荐）
.\scripts\ci-local.ps1      # Windows → ci-windows-gate；其他 OS → ci-pr-gate
.\scripts\run_tests.ps1     # 仅后端 nextest（--profile ci）
```

前端：`cd ui && npm test && npm run build`。详见 [docs/README.md](docs/README.md) CI 节。

---

## 文档

| 文档 | 内容 |
|------|------|
| [FRAMEWORK.md](FRAMEWORK.md) | 架构分层、数据流、Fork/压缩/IPC 等技术细节 |
| [docs/README.md](docs/README.md) | Crate 专题索引与阅读路径 |
| [prompt/system.md](prompt/system.md) | Agent 行为与创作规范（运行时嵌入） |

---

## 界面简述

**两栏主区：** **文件树** · **聊天**（`TranscriptView` 分段渲染；SubAgent 经 `SubAgentForkCard` 进入 overlay）。**StatusBar** 提供作品/会话切换、Todo 下拉、权限与模型选择、Token 统计与子 Agent 状态。Settings 为弹窗面板。

聊天区支持流式回复、工具卡片批准/拒绝、AskUserQuestion 问答、SubAgent 详情 overlay。Turn 进行中（流式、待批准工具、待回答问题）时模型与权限选择器禁用。

**会话（StatusBar）：**

| 操作 | 说明 |
|------|------|
| 下拉切换 | `resume_session` 恢复历史；**仅查看/切换不会刷新「最后活跃」时间** |
| `+` 新建 | `create_session`，当前作品下空白会话 |
| 标签 | `{标题} · 对话 N 轮 · {相对时间} · {模型}` |

会话列表按最近 LLM 活跃时间降序排列。StatusBar 展示会话累计 token 三分类与当前上下文（DB `context_tokens`，最近一次 API 快照；经轮询刷新）。详见 [FRAMEWORK.md §2.2](FRAMEWORK.md#22-作品与会话)。
