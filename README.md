# DeepSeek Novel Agent

面向长篇小说创作的 **AI 协作桌面应用**。作者与 Agent 多轮对话，在作品目录内完成策划、写章、改稿与设定维护。知识库、章节正文与跨会话记忆均以 Markdown 文件落盘，可用 Git 管理、人工审阅。

基于 Rust + Tauri + React 构建，对接 **DeepSeek V4 Pro / V4 Flash**（百万上下文、流式工具调用）。前端一键切换模型，无需重启。

**核心特点：** 创作流程按大纲→细纲→正文逐级推进，Workflow Skill 约束各阶段 SOP。写章分两层审计：**细纲完成后** Fork PlanAuditor 检查计划质量（大纲对齐、伏笔密度、因果闭合等）；**正文写完后**并行 Fork KnowledgeAuditor + ChapterCraftAnalyzer 检查执行忠实度与文笔一致性。子 Agent 在独立上下文中跑完，主 Agent 按报告修复后再宣告完成。详见 [FRAMEWORK.md](FRAMEWORK.md)。

---

## 能做什么

| 能力 | 说明 |
|------|------|
| **作品管理** | 多作品并存，各自独立的知识库、章节与创作历史 |
| **策划** | 大纲 → 细纲 → 人物卡 → 伏笔与因果链，逐级细化 |
| **写章** | 按细纲撰写正文，写后自动同步角色/场景/伏笔追踪表 |
| **改稿** | 影响分析 + 级联修改正文与关联设定 |
| **质量检查** | 细纲后 PlanAuditor（计划结构）；正文后 KnowledgeAuditor + ChapterCraftAnalyzer（设定一致、对话节奏、伏笔衔接） |
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

作品数据在 `works/` 下，与 Agent 代码分离。切换作品时前端同步切换会话库与文件树。跨会话**审计台账**在 `{作品}/knowledge/meta/audit-status.md`（Agent 可读）；引擎调试 JSONL 在 `{作品}/.novel/logs/`（非 Agent 知识层）。

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

**前置**

- [Rust](https://rustup.rs)（含 `cargo`）
- **Node.js 24**（见 `ui/.nvmrc`；在 `ui/` 下执行 `nvm use` 或 `fnm use`）
- [Tauri 系统依赖](https://v2.tauri.app/start/prerequisites/)（Windows 需 WebView2）

以下命令均在**仓库根目录**执行（需含 `skills/`、`templates/` 目录，克隆后即存在）。

**首次**安装前端依赖（`cargo tauri dev` 不会自动执行 `npm install`）：

```bash
npm --prefix ui install
```

**开发模式（推荐）：** 依赖装好后，日常只需：

```bash
cargo tauri dev
```

自动编译 Rust、启动 Vite 并打开桌面窗口。修改 `crates/` 或 `src-tauri/` 后需重启；仅改 `ui/` 由 Vite 热更新，无需重启。

**仅编译可执行文件（不打安装包）：**

```bash
npm --prefix ui run build   # 首次或改 ui 后需要
cargo build --release -p novel-agent
```

产物：`target/release/novel-agent.exe`（workspace 根目录下的 `target/`，非 `src-tauri/target/`）。请在仓库根目录启动，并保留 `skills/`、`templates/` 布局。

**构建安装包：**

```bash
cargo tauri build --bundles nsis
```

安装包输出于 `target/release/bundle/`（如 `nsis/*-setup.exe`）。默认 `cargo tauri build`（`targets: all`）在 Windows 还会打 MSI，需从 GitHub 下载 WiX/NSIS；国内网络易出现 `timeout: global`。可只打 NSIS（上式），或配置 `HTTP_PROXY`/`HTTPS_PROXY` 后重试；仅需本地运行时直接用上面的 `novel-agent.exe` 即可。

**API Key（任选其一）：**

- 环境变量 `DEEPSEEK_API_KEY`（优先级最高）
- 应用内 Settings → 自动写入 `.novel-agent/api_config.json`
- 均未配置时使用离线 mock（无真实 LLM 调用）

可选环境变量：`DEEPSEEK_API_BASE`、`NOVEL_MODEL`、`NOVEL_COMPACTION_THRESHOLD` 等，详见 [novel-config](docs/crates/novel-config.md)。

**测试与 CI：**

```powershell
.\scripts\ci-windows.ps1  # Windows 本地全量（含 audit；GHA 见 docs/README.md CI 矩阵）
.\scripts\ci-local.ps1    # 跨平台本地 CI 入口
```

详见 [scripts/README.md](scripts/README.md) 与 [docs/README.md](docs/README.md) CI 节。

---

## 文档

| 文档 | 内容 |
|------|------|
| [FRAMEWORK.md](FRAMEWORK.md) | 架构分层、数据流、Fork/压缩/IPC 等技术细节 |
| [docs/README.md](docs/README.md) | Crate 专题索引与阅读路径 |
| [prompt/system.md](prompt/system.md) | Agent 行为与创作规范（运行时嵌入） |

---

## 界面简述

**布局：** 左侧**文件树**浏览作品目录，右侧**聊天区**与 Agent 对话。顶部状态栏左侧为**待办**、作品与会话切换、Token 用量；右侧为**设置**（弹窗）。

**聊天区：**

- 你与 Agent、子任务检查的回复均以气泡展示；需要确认的工具操作、选择题会单独成卡片
- 子 Agent（如写后审计）在独立浮层中查看，不挤占主对话
- 当前这一轮对话占满可视区域，往上滚可看更早记录
- 历史消息按需加载，长会话不会一次占满内存；回到底部后只保留最近几轮在内存中
- 回复边生成边显示；Agent 要向你提问时，会暂停并等你作答

**待办（状态栏）：** Agent 写入待办后会即时出现在「待办事项」里。有未完成项时按钮高亮并显示数量，新增待办时自动展开、全部完成后自动收起。列表按**进行中 / 未进行 / 已完成**分组，已完成项显示删除线。点击条目可在三种状态间切换。

Turn 进行中（流式、待批准工具、待回答问题）时模型与权限选择器禁用。

**会话（StatusBar）：**

| 操作 | 说明 |
|------|------|
| 下拉切换 | `resume_session` 恢复历史；**仅查看/切换不会刷新「最后活跃」时间** |
| `+` 新建 | `create_session`，当前作品下空白会话 |
| 标签 | `{标题} · 对话 N 轮 · {相对时间} · {模型}` |

会话列表按最近 LLM 活跃时间降序排列。StatusBar 展示会话累计 token 三分类与当前上下文（DB `context_tokens`，最近一次 API 快照）；运行中由 **`session-tokens-updated` 事件驱动**（主/SubAgent 每次 LLM 调用后推送），初始与切 session 经 `get_app_status`。详见 [FRAMEWORK.md §2.2](FRAMEWORK.md#22-作品与会话)。
