# DeepSeek Novel Agent

面向长篇小说创作的 **AI 协作桌面应用**。作者与 Agent 多轮对话，在作品目录内完成策划、写章、改稿与设定维护。知识库、章节正文与跨会话记忆均以 Markdown 文件落盘，可用 Git 管理、人工审阅。

基于 Rust + Tauri + React 构建，对接 **DeepSeek V4 Pro / V4 Flash**（百万上下文、流式工具调用）。前端一键切换模型，无需重启。

## 核心能力

| 能力 | 说明 |
|------|------|
| **作品管理** | 多作品并存，各自独立的知识库、章节与创作历史 |
| **策划** | 大纲 → 细纲 → 人物卡 → 伏笔与因果链，逐级细化 |
| **写章** | 按细纲撰写正文，写后自动同步角色/场景/伏笔追踪表 |
| **改稿** | 影响分析 + 级联修改正文与关联设定 |
| **质量检查** | 策划后 `audit-plan`；正文后 `audit-knowledge` + `audit-craft` |
| **流派扩展** | 30+ 题材 Skill（仙侠、科幻、快穿等）按需加载 |
| **权限模式** | 常规 / 策划 / 自动 / 无人值守，控制写操作是否需确认 |

Agent 在作品目录 sandbox 内读写 `knowledge/`、`chapters/`、`memory/`。Graph 编排约束创作顺序，节点内 Workflow Skill 提供 SOP，确保先大纲后正文、写后必审计。

架构细节（Graph-Primary 编排、Book Loop、Fork 子 Agent、IPC 事件流等）见 **[FRAMEWORK.md](FRAMEWORK.md)**。

## 快速开始

### 前置

- [Rust](https://rustup.rs)（含 `cargo`）
- **Node.js 24**（见 `ui/.nvmrc`）
- [Tauri 系统依赖](https://v2.tauri.app/start/prerequisites/)（Windows 需 WebView2）

以下命令均在**仓库根目录**执行。

### 安装与运行

```bash
# 首次：安装前端依赖（cargo tauri dev 不会自动执行）
pnpm --prefix ui install

# 开发模式（Vite HMR，改 ui/ 无需重启）
cargo tauri dev
```

### 构建

```bash
# 仅编译可执行文件
pnpm --prefix ui run build
cargo build --release -p novel-agent
# 产物：target/release/novel-agent.exe

# 构建安装包（NSIS）
cargo tauri build --bundles nsis
```

请在仓库根目录启动，确保 `skills/`、`templates/` 目录存在。

### API Key

任选其一（优先级从高到低）：

- 环境变量 `DEEPSEEK_API_KEY`
- 应用内 Settings → 自动写入 `.novel-agent/api_config.json`
- 均未配置时使用离线 mock（无真实 LLM 调用）

可选环境变量：`DEEPSEEK_API_BASE`、`NOVEL_MODEL`、`NOVEL_COMPACTION_THRESHOLD` 等，详见 [novel-config](docs/crates/novel-config.md)。

### 测试

```powershell
.\scripts\ci-windows.ps1   # Windows 本地全量
.\scripts\ci-local.ps1     # 跨平台本地 CI 入口
```

详见 [scripts/README.md](scripts/README.md) 与 [docs/README.md](docs/README.md) CI 节。

## 文档导航

| 文档 | 适合 |
|------|------|
| [FRAMEWORK.md](FRAMEWORK.md) | 架构分层、数据流、Graph-Primary、Fork/压缩/IPC、前端状态管理 |
| [docs/README.md](docs/README.md) | Crate 专题索引、阅读路径、UI 概要、CI/CD |
| [docs/crates/novel-graph.md](docs/crates/novel-graph.md) | Graph 编排 SSOT：plan schema、GraphTracker、Book Loop、handoff、gate |
| [prompt/shared-base.md](prompt/shared-base.md) | 共享底座：工具约定、权限、Memory（所有 Agent 共用） |
| [prompt/orchestrator.md](prompt/orchestrator.md) | 图编排器：PlanBuilder、节点激活协议、Gate 评估 |

## 项目结构

```
novel_agent/
├── crates/                # Rust 后端（10 个业务 crate + novel-server）
├── src-tauri/             # Tauri 桌面壳
├── ui/                    # React 18 + TypeScript + Vite 8 前端
├── skills/                # Agent 级 Workflow + 流派 Skill
├── templates/             # 新建作品脚手架（运行时必填）
├── prompt/                # System 与子 Agent 提示词
├── works/{作品名}/         # 用户作品实例（gitignore）
│   ├── knowledge/         # 知识库 + plan-graph.json 编排
│   ├── chapters/          # 章节正文
│   ├── memory/            # 跨会话记忆
│   └── .novel-agent/      # 作品级 state.db
├── docs/                  # Crate 专题文档
└── scripts/               # CI / 构建 / 工具脚本
```

完整目录布局与数据归属见 [FRAMEWORK.md §1.2](FRAMEWORK.md#12-agent-根目录与数据归属)。

## 界面一览

左侧**文件树**浏览作品目录；主舞台为 **Chat** 聊天区。状态栏提供作品/会话切换、待办事项、Token 用量、Graph 画布入口。点击 Graph 节点进入该节点专属会话，按 Waiting → Running → Achieved 状态流转。审计 Skill 在节点内 Invoke 执行，可选 Fork 隔离查看。

UI 交互细节（Graph 画布、聊天区布局、Turn 懒加载、Compaction 横幅等）见 [FRAMEWORK.md §2.5](FRAMEWORK.md#25-前端状态与-ipc) 与 [docs/README.md §前端 UI 概要](docs/README.md#前端-ui-概要)。

## 其他

**清理作品会话库：** 删除 `works/**/.novel-agent/state.db*`（不影响正文与知识库）。脚本：`scripts/reset-work-databases.ps1`（或 `.sh`）。详见 [docs/README.md §清理](docs/README.md#清理作品会话库)。
