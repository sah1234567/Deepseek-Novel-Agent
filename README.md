# DeepSeek Novel Agent

面向长篇小说创作的 **AI 协作桌面应用**。作者与 Agent 多轮对话，在作品目录内完成策划、写章、改稿与设定维护；知识库、章节正文与跨会话记忆均以文件落盘，便于版本管理与人工审阅。

技术栈：Rust + Tauri + React；默认对接 **DeepSeek V4 Pro**（百万上下文、KV cache 命中低价、流式工具调用）。

**架构要点：** 针对 V4 Pro 的 prefix cache 计费，主会话需保持 system prompt 与对话前缀尽量稳定。因此 **Fork SubAgent** 只用于**专项并行审计**（写后/改稿后的 KnowledgeAuditor + ChapterCraftAnalyzer）——子 Agent 在独立上下文里跑完，主 Agent 只收一条摘要，完整 transcript 不进主 LLM。日常策划、写章、改稿所需的 Read / Tail / Grep 等仍由**主 Agent**直接调用（读盘经济见 `prompt/system.md`）。默认模型 `deepseek-v4-pro`；Fork 细节见 [FRAMEWORK.md §2.3](FRAMEWORK.md#23-fork-子-agent)。

---

## 能做什么

| 能力 | 说明 |
|------|------|
| **作品管理** | 多作品并存（`works/{作品名}/`），各自独立知识库、章节与 SQLite 会话 |
| **策划** | 大纲、细纲、人物卡、伏笔与因果链；Plan 模式下草案可写入 `plan/` |
| **写章** | 按细纲撰写 2000–4000 字/章；写后同步知识库演变日志 |
| **改稿** | 影响分析 + 级联修改正文与设定 |
| **质量检查** | 子 Agent 并行审计（KnowledgeAuditor、ChapterCraftAnalyzer） |
| **流派扩展** | 30+ 题材 Skill（仙侠、科幻、快穿等）按需加载 |
| **权限模式** | 常规 / 策划 / 自动 / 无人值守，控制 Write/Edit 是否需作者确认 |

Agent 在作品目录 sandbox 内读写 `knowledge/`、`chapters/`、`memory/`；系统提示词与 Workflow Skill 约束创作顺序（先大纲 → 细纲 → 正文），避免跳步写作。

---

## 目录概览

```
novel_agent/
├── works/{作品名}/          # 用户作品（知识库、章节、settings、state.db）
├── skills/                  # Agent 级 Skill（作品可在 works/{名}/skills/ 覆盖同 id）
├── templates/               # 新建作品脚手架（必填）
├── prompt/                  # System 与子 Agent 提示词
└── .novel-agent/            # 全局 API 配置等
```

作品数据在 `works/` 下，与 Agent 代码分离；切换作品时前端同步切换会话库与文件树。

---

## 快速开始

**前置：** [Rust](https://rustup.rs)、[Node.js](https://nodejs.org)、Tauri 系统依赖（Windows 需 WebView2）；克隆/解压后 Agent 根目录下须有 `templates/` 与 `skills/`。

**首次运行（开发模式，推荐）：**

```bash
cd novel_agent
cd ui && npm install && cd ..
cargo tauri dev
```

`tauri dev` 会自动编译 Rust、启动前端并打开桌面窗口。改 Rust 代码后需**重启**该命令才能生效；只改 `ui/` 则 Vite 热更新。

**本地安装包（Release，下载后想直接装/跑）：**

```bash
cd ui && npm install && cd ..
cargo tauri build
```

产物在 `src-tauri/target/release/`（可执行文件）及 `src-tauri/target/release/bundle/`（安装包，视平台而定）。

**API Key（任选其一）：**

- 环境变量 `DEEPSEEK_API_KEY`（优先）
- 应用内 Settings 写入 `.novel-agent/api_config.json`
- 未配置时使用离线 mock（无真实 LLM）

可选环境变量：`DEEPSEEK_API_BASE`、`NOVEL_MODEL`、`NOVEL_COMPACTION_THRESHOLD` 等（见 [novel-config](docs/crates/novel-config.md)）。

**测试：**

```powershell
.\scripts\run_tests.ps1    # Windows
# cargo test --workspace   # 跨平台
```

---

## 文档

| 文档 | 内容 |
|------|------|
| [FRAMEWORK.md](FRAMEWORK.md) | 架构分层、数据流、Fork/压缩/IPC 等技术细节 |
| [docs/README.md](docs/README.md) | Crate 专题索引与阅读路径 |
| [prompt/system.md](prompt/system.md) | Agent 行为与创作规范（运行时嵌入） |

---

## 界面简述

三栏：**文件树** · **待办** · **聊天**。StatusBar 提供作品/会话切换、权限模式、Token 统计与子 Agent 状态。聊天区支持流式回复、工具卡片批准/拒绝、AskUserQuestion 问答、SubAgent 详情 overlay。

实现细节见 [FRAMEWORK.md §1.5](FRAMEWORK.md#15-前后端边界) 与 [docs/README.md §前端 UI](docs/README.md#前端-ui-概要)。
