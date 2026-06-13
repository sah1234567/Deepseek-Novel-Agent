# novel-config — 配置管理

> 所属项目: [Novel Agent](../../README.md)

---

## 1. 业务逻辑

### 1.1 路径布局

| 路径 | 说明 |
|------|------|
| `{agent_root}/templates/` | 新建作品脚手架 Markdown 模板 |
| `{agent_root}/skills/` | Agent 级 Skill（固定，不随作品切换） |
| `{agent_root}/works/{name}/` | 作品根目录 |
| `{agent_root}/.novel-agent/api_config.json` | 全局 API Key / Base URL |
| `{work}/.novel-agent/state.db` | 作品级 SQLite（sessions/messages/todos） |
| `{work}/settings.json` | 作品级模型与 Hook 配置 |

`resolve_agent_root()` 从可执行文件或 cwd 向上查找含 `skills/` 的目录。

**路径 API：** `works_dir`, `skills_dir`, `templates_dir`, `work_path`, `validate_work_name`, `ensure_work_under_works`, `global_api_config_path`, `global_config_dir`。

### 1.2 配置分层

**作品 settings.json：** 默认值 → JSON → 环境变量（figment）

**API Key 优先级：** `DEEPSEEK_API_KEY`（env）> `{agent_root}/.novel-agent/api_config.json` > 离线 mock

API Key **不在** per-work `settings.json` 中持久化。全局读写：`load_agent_api_config` / `save_agent_api_config` / `AgentApiConfig`（全局 `api_config.json`，`state.db` 内旧表已移除）。运行时解析：`resolve_agent_api_key` / `resolve_agent_api_base`（`DEEPSEEK_*` env 优先，供 LLM 与 WebSearch 共用）。

### 1.3 settings.json 结构

**project：** title, author, genre[], language

**model：** provider (deepseek), model, api_base, context_window_size (1M), compaction_threshold (0.8), max_output_tokens, thinking_enabled (default true)

**hooks：** `post_tool_use: HookMatcher[]`

**permissions：** mode, deny_rules, always_allow

**agent：** knowledge_auditor_max_react_loops（默认见 `fork_agents::KNOWLEDGE_AUDITOR_MAX_REACT_LOOPS_DEFAULT`）、max_tool_concurrency

**fork_agents（`fork_agents.rs`）：** `FORKABLE_AGENT_TYPE_NAMES` 与默认 ReAct 上限常量；`ForkSubAgent` JSON schema 与 `novel-core::FORK_AGENT_CATALOG` 共用名称层。

### 1.4 Hook 配置结构

```json
{
  "hooks": {
    "post_tool_use": [{
      "matcher": "Write(chapters/**)|Edit(chapters/**)",
      "hooks": [{
        "type": "agent",
        "prompt": "检查演变日志最后一行…",
        "timeout": 60
      }]
    }]
  }
}
```

- `HookMatcher.matcher` — 工具名 + 可选路径模式
- `HookRule.hook_type` — `"agent"` 或 `"prompt"`
- `HookRule.prompt` — 注入 KnowledgeAuditor hook task 的指令
- 代码默认：`novel-core::hooks::default_hook_config()`

### 1.5 配置加载

`load_project_settings(path)` — figment 合并：默认值 → JSON → 环境变量

`ProjectSettings::validate()` — 范围校验（context_window_size > 0, compaction_threshold [0, 1], max_tool_concurrency [1, 32]）

### 1.6 ModelConfig

- Provider：Deepseek（当前唯一支持的 provider）
- 默认 api_base：`https://api.deepseek.com/v1`
- 默认 context_window_size：1,000,000
- `thinking_enabled`：控制 DeepSeek reasoning 模式（默认 true），可通过 `NOVEL_THINKING_ENABLED` 环境变量覆盖

支持的环境变量覆盖：`NOVEL_API_BASE`、`NOVEL_MODEL`、`NOVEL_COMPACTION_THRESHOLD`、`NOVEL_THINKING_ENABLED`、`NOVEL_MAX_OUTPUT_TOKENS`、`NOVEL_CONTEXT_WINDOW_SIZE`
