# novel-memory — 记忆编排

> 所属项目: [Novel Agent](../../README.md)
>
> **说明：** 记忆类型定义、文件扫描、选择（`MemorySelector` trait / DIP）、提取、预取、fork 权限门控均在本 crate 自包含。YAML frontmatter 解析（`frontmatter`）、UTF-8 截断（`memory_types`）已内联，文件读写用 `std::fs`。不依赖 `novel-knowledge`。

---

## 1. 业务逻辑

`novel-memory` 统一管理**作品记忆**的全生命周期：5 种封闭类型（`style` / `plot_decision` / `character_guardrail` / `feedback` / `reference`），从 `memory/` 目录扫描、LLM 选择注入、背景提取到 fork 权限守卫。

### 1.1 模块划分

| 模块 | 职责 |
|------|------|
| `frontmatter` | YAML frontmatter 解析（`parse_frontmatter`）——自包含，替代 `novel-knowledge::parser` |
| `memory_types` | `MemoryType` / `MemoryHeader` / `MemoryConstants` / `MemoryStatus` / `SurfacedMemory` 等类型；共享工具 `truncate_memory_body`、`truncate_bytes_utf8`、`memory_type_description` |
| `memory_scan` | 遍历 `memory/` 目录（递归子目录），解析 YAML frontmatter，过滤 `deprecated`/`superseded`（`MemoryStatus::is_active()`），按 mtime 排序，格式化为 LLM selection manifest |
| `memory_select` | 选择器 prompt 常量（`SELECT_MEMORIES_SYSTEM_PROMPT`、`SELECT_MEMORIES_SCHEMA`）、`parse_selector_response`（JSON → `Vec<String>`）——均为 **crate-internal** |
| `memory_extract` | `MemoryExtractor` 状态机：游标追踪、节流门控（`pass_throttle_gate`）、进行中归并（`coalesce_pending`）、`try_prepare_extraction` → `complete_extraction` |
| `memory_extract_prompt` | 提取子 Agent 的任务 prompt 构建（嵌入 `prompt/memory/extraction-task.md`） |
| `loading` | `load_memory`：将记忆文件加载到静态 system prompt（表头由 `MEMORY_TYPES` 数据驱动） |
| `selection` | `MemorySelector` trait（DIP）+ `select_relevant` 管线 + `ChatClient` 实现 + `create_selector_from_config`（composition root） |
| `prefetch` | `MemoryPrefetch`：后台 `scan → select → read` 管线；`collect_surfaced_paths` 去重；`format_attachment` 格式化注入 |
| `guard` | `memory/` 路径守卫（`is_memory_rel_path` / `is_memory_write_tool`）+ 写入检测（`has_memory_writes_since`）+ fork 权限（`memory_fork_can_use_tool`） |

### 1.2 记忆类型系统（`memory_types`）

| 类型 | 中文标签 | 存储内容 |
|------|----------|----------|
| `style` | 文风 | 偏好、节奏、描写习惯 |
| `plot_decision` | 剧情决策 | 不可逆剧情决策与理由 |
| `character_guardrail` | 人物禁区 | 角色绝不能做的事 |
| `feedback` | 反馈 | 外部反馈、读者意见、确认的模式 |
| `reference` | 参考 | 外部参考、灵感来源、对标作品 |

**状态生命周期（`MemoryStatus`）：**

| 状态 | 含义 | is_active() |
|------|------|-------------|
| `Active` | 当前有效 | `true` |
| `Superseded` | 被新决策替代（保留原文件，标记为 superseded） | `false` |
| `Deprecated` | 不再适用（保留原文件——阻止 Agent 重新提出） | `false` |

`MemoryStatus::is_active()` 封装状态判断，`memory_scan` 和 `format_memory_manifest` 均通过它过滤而非手动枚举。

**内存常量（`MemoryConstants`）：**

| 常量 | 值 | 用途 |
|------|----|------|
| `FRONTMATTER_MAX_LINES` | 30 | 扫描时只读前 N 行 |
| `MAX_MEMORY_FILES` | 200 | 最多扫描文件数（mtime 排序） |
| `MAX_MEMORY_LINES` / `MAX_MEMORY_BYTES` | 200 / 4096 | 单条记忆注入上限 |
| `MAX_SESSION_BYTES` | 60KB | 每会话累计注入上限 |
| `MEMORY_PREFETCH_MIN_WORDS` | 4 | 触发预取的最低用户输入词数（CJK/ASCII 混合启发式） |
| `EXTRACTION_THROTTLE_TURNS` | 1 | 提取节流周期（1 = 每轮） |
| `FLASH_MAX_TOKENS` | 256 | Flash 选择器输出 token 上限 |

### 1.3 记忆加载（`load_memory`）

每轮构建动态 system prompt 时调用：

1. 扫描 `memory/` 子目录（5 种类型），解析 YAML frontmatter
2. 通过 `MemoryStatus::is_active()` 过滤 deprecated/superseded
3. 加载 body 内容，单条 ≤ `max_bytes`（默认 4096），累计 ≤ 总量上限
4. 格式化为 Markdown 表格 + 条目注入 system prompt（表头由 `MEMORY_TYPES` 数据驱动，`memory_type_description` 提供中文描述）

```rust
pub fn load_memory(project_root: &Path, max_bytes: usize) -> String;
```

### 1.4 记忆选择（`MemorySelector` trait + `select_relevant`）

依赖反转（DIP）：`MemorySelector` trait 将选择逻辑与具体 `ChatClient` 解耦。

```rust
#[async_trait]
pub trait MemorySelector: Send + Sync {
    async fn side_query(
        &mut self, system: &str, user_message: &str,
        max_tokens: u32, response_format: Option<Value>,
    ) -> Result<SideQueryResult, LlmError>;
}

impl MemorySelector for ChatClient;

pub async fn select_relevant(
    selector: &mut impl MemorySelector, query: &str,
    memories: &[MemoryHeader],
) -> Result<Vec<String>, LlmError>;
```

选择流程：候选记忆 → manifest 格式化 → Flash side query（V4 Flash，256 token）→ JSON 解析 → 文件名验证 → 最多 5 个。测试可用 `MockSelector` 实现 trait，无需真实 API Key。

`create_selector_from_config` 是 composition root，从 `ModelConfig::memory_selector()` 与 `api_config.json` 创建 `ChatClient`。

### 1.5 记忆预取（`MemoryPrefetch`）

背景 `scan → select → read` 管线，与主 LLM 流式响应并行：

| 阶段 | 方法 | 说明 |
|------|------|------|
| Start | `MemoryPrefetch::start(selector, query, dir, surfaced)` | spawn `tokio::task` 后台执行 |
| Dedup | `collect_surfaced_paths(messages)` | 扫描已注入的 `Memory (记录于 ChN): path:` 模式 |
| Cap | `count_surfaced_bytes(paths, dir)` | 累计字节达 `MAX_SESSION_BYTES` 则跳过 |
| Consume | `prefetch.consume().await` | 阻塞等待结果 |
| Inject | `format_attachment(memory)` | 格式化为 `Memory (记录于 …): …:` 附件 |

在 `AgentEngine::handle_message_with_events` 中集成：turn 开始 spawn → turn 结束 consume → 注入为 user 消息。

### 1.6 记忆提取（`MemoryExtractor`）

fire-and-forget 子 Agent，每轮结束后检查是否有新对话可提取为记忆文件。

**门控顺序（`try_prepare_extraction`）：**

1. 主 Agent 本轮已写 `memory/` → 跳过（避免重复提取）
2. 节流门控（`pass_throttle_gate`）—— `EXTRACTION_THROTTLE_TURNS` 控制频率
3. 已有提取进行中 → 归并待处理 context（`coalesce_pending`）
4. 无新消息 → 跳过
5. CAS 获取进行中锁 → 构建 prompt

`complete_extraction` 处理归并的 trailing job 链式调度。

```rust
pub struct MemoryExtractor;
impl MemoryExtractor {
    pub fn new() -> Self;
    pub fn cursor(&self) -> usize;
    pub fn should_run(&self, ctx: &ExtractionContext) -> bool;
    pub fn try_prepare_extraction(&self, ctx: &ExtractionContext) -> Option<PreparedMemoryExtraction>;
    pub fn complete_extraction(&self, msg_count: usize) -> Option<PreparedMemoryExtraction>;
    pub fn reset(&self);
}

pub struct ExtractionContext {
    pub message_count: usize,
    pub project_root: PathBuf,
    pub main_agent_wrote_memory: bool,
}
```

### 1.7 Fork 权限守卫（`guard`）

记忆提取子 Agent 的权限模型（由 `novel-tools::subagent_gate` 调用）：

| 工具 | 允许范围 |
|------|----------|
| `Read` / `Grep` / `Glob` | 任意路径 |
| `Write` / `Edit` | 仅 `memory/` 目录内 |
| `Bash` / `ForkSubAgent` / `TodoWrite` / … | 禁止 |

```rust
pub fn is_memory_rel_path(path: &str) -> bool;            // path 在 memory/ 内
pub fn is_memory_write_tool(tool: &str, input: &Value) -> bool;  // Write/Edit 目标 memory/
pub fn memory_fork_can_use_tool(tool: &str, path: Option<&str>) -> bool;
pub fn has_memory_writes_since<'a>(calls: impl IntoIterator<Item = (&'a str, &'a Value)>) -> bool;
```

`has_memory_writes_since` 解耦自 `novel-core::ChatMessage`——接受 `(tool_name, tool_arguments)` 对。

### 1.8 外部依赖

| 依赖 | 用途 |
|------|------|
| `novel-deepseek` | `ChatClient`（`MemorySelector` 默认实现）、`LlmError`、`TokenUsage` |
| `novel-config` | `ModelConfig::memory_selector()` 获取 Flash 模型配置；`resolve_agent_api_key` / `resolve_agent_api_base` |
| `serde` / `serde_json` / `serde_yaml` | frontmatter 反序列化、selection JSON schema、响应解析 |
| `async-trait` | `MemorySelector` trait 的 `async fn` |
| `tokio` / `tracing` | 异步运行时、日志 |