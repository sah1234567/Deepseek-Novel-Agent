---
name: engineering-principles
description: >-
  重构或新增 feature（Rust / TypeScript）时遵循的软件工程基本原则：
  DRY（不要重复自己）、单一职责与高内聚、正交、最小暴露与接口隔离、
  开闭原则、里氏替换原则、依赖反转原则、清晰注释。
  用于架构决策、代码拆分、接口设计、抽象边界审查。
---

# 软件工程基本原则

在本项目内重构或新增 Rust（`crates/`）或 TypeScript（`ui/`）代码时，必须在设计和审查阶段遵循以下原则。每个原则包含**判断标准**、**Rust 正反例**、**TypeScript 正反例**。末尾的「重构执行流程」和「检查清单」确保改动严格满足所有要求。

---

## 1. DRY — 不要重复自己

**判断：** 同一知识/逻辑/规则在系统中有且仅有一处表示。注意区分的不是"文本相似"而是"语义相同"——两个函数写起来像但服务于不同业务目标、有不同变化节奏时，合并反而是耦合。

### Rust

```rust
// ❌ 两个 crate 各自定义语义相同的错误
// crates/novel-core/src/error.rs
pub enum CoreError { WorkNotFound(String) }
// crates/novel-server/src/error.rs
pub enum ServerError { WorkNotFound(String) }

// ✅ 公共类型统一定义，各 crate 通过 #[from] 组合
// crates/novel-tools/src/error.rs
#[derive(Debug, thiserror::Error)]
pub enum AgentError {
    #[error("作品不存在: {0}")]
    WorkNotFound(String),
}
// crates/novel-core/src/error.rs —— 只定义 Core 独有的变体
#[derive(Debug, thiserror::Error)]
pub enum CoreError {
    #[error(transparent)]
    Agent(#[from] AgentError),
}
```

### TypeScript

```typescript
// ❌ 两个组件各自实现相同的 Tauri listen 逻辑
function ChatPanel() {
  const [msgs, setMsgs] = useState<Message[]>([]);
  useEffect(() => {
    const u = listen<MsgEvent>('new-msg', e => setMsgs(p => [...p, e.payload]));
    return () => { u.then(f => f()); };
  }, []);
}
function NotificationBar() {
  const [msgs, setMsgs] = useState<Message[]>([]);  // 完全相同的逻辑
  useEffect(() => {
    const u = listen<MsgEvent>('new-msg', e => setMsgs(p => [...p, e.payload]));
    return () => { u.then(f => f()); };
  }, []);
}

// ✅ 抽取为自定义 hook —— 单一声源
function useIncomingMessages() {
  const [msgs, setMsgs] = useState<Message[]>([]);
  useEffect(() => {
    const u = listen<MsgEvent>('new-msg', e => setMsgs(p => [...p, e.payload]));
    return () => { u.then(f => f()); };
  }, []);
  return msgs;
}
```

---

## 2. 单一职责与高内聚

**判断：** 一个模块/函数只有一个引起变化的原因。如果描述这个函数做了什么需要 "和" 字连接，就该拆分。高内聚是其内部可观察结果——所有方法围绕同一组数据、同一个目标。

### Rust

```rust
// ❌ 一个函数串联四层：解析→调 LLM→写库→发事件
fn handle_user_message(msg: &str, db: &SessionDb) -> Result<Response> {
    let parsed = parse_message(msg)?;
    let llm_resp = call_deepseek(&parsed)?;
    db.save_turn(&parsed, &llm_resp)?;
    emit_event("turn-complete", &llm_resp)?;
    Ok(llm_resp)
}

// ✅ 每层独立，上层编排
fn parse_and_call(msg: &str) -> Result<(ParsedMessage, LlmResponse)> {
    let parsed = parse_message(msg)?;
    let resp = call_deepseek(&parsed)?;
    Ok((parsed, resp))
}
fn persist_and_emit(p: &ParsedMessage, r: &LlmResponse, db: &SessionDb) -> Result<()> {
    db.save_turn(p, r)?;
    emit_event("turn-complete", r)?;
    Ok(())
}
```

### TypeScript

```typescript
// ❌ 一个组件管理数据加载 + 事件监听 + UI 渲染
function WorkDetail({ name }: { name: string }) {
  const [detail, setDetail] = useState<WorkDetail | null>(null);
  const [loading, setLoading] = useState(true);
  const [status, setStatus] = useState<WorkStatus>('idle');
  useEffect(() => { invoke('get_detail', { name }).then(setDetail).finally(() => setLoading(false)); }, [name]);
  useEffect(() => {
    const u = listen<StatusEvent>('status', e => setStatus(e.payload.status));
    return () => { u.then(f => f()); };
  }, [name]);
  if (loading) return <Spinner />;
  return <div><StatusBadge status={status} /><h1>{detail.title}</h1></div>;
}

// ✅ 逻辑抽到 hook，组件只渲染
function useWorkDetail(name: string) { /* 数据加载 */ return { detail, loading }; }
function useWorkStatus(name: string) { /* 事件监听 */ return status; }
function WorkDetail({ name }: { name: string }) {
  const { detail, loading } = useWorkDetail(name);
  const status = useWorkStatus(name);
  if (loading) return <Spinner />;
  return <div><StatusBadge status={status} /><h1>{detail.title}</h1></div>;
}
```

---

## 3. 正交

**判断：** "修改模块 A 是否需要同时修改 B、C、D？"需要则不正交。本项目中 `novel-core` 与 `novel-server` 通过 `EngineCommand`/`Event` 枚举交互，前端与后端通过 Tauri `invoke` 通信——双方可独立演化。

### Rust

```rust
// ❌ novel-server 直接读取 core 内部字段——core 改名，server 炸
fn build_response(engine: &EngineState) -> String {
    format!("turn: {}", engine.inner.turn_count)  // 越过 pub API 访问内部
}

// ✅ 通过公开查询方法访问——内部变化不影响调用方
impl EngineState {
    pub fn turn_count(&self) -> usize { self.inner.turn_count }
}
fn build_response(engine: &EngineState) -> String {
    format!("turn: {}", engine.turn_count())
}
```

### TypeScript

```typescript
// ❌ UI 组件直接耦合 Tauri IPC——无法单独测试
function SaveButton({ name }: { name: string }) {
  return <button onClick={() => invoke('save', { name })}>Save</button>;
}

// ✅ 通过回调解耦——Button 是纯 UI，业务逻辑由父级注入
function SaveButton({ onSave }: { onSave: () => void }) {
  return <button onClick={onSave}>Save</button>;
}
function WorkPage({ name }: { name: string }) {
  return <SaveButton onSave={() => invoke('save', { name })} />;
}
```

---

## 4. 最小暴露与接口隔离

**判断：** 从提供方问 "外部真的需要这个吗？"（最小暴露）；从消费方问 "我真的需要这个接口的所有方法吗？"（接口隔离）。两者互为补充。

### Rust

```rust
// ❌ struct 字段全部 pub——外部可任意修改内部状态
pub struct SessionConfig {
    pub db_path: PathBuf,
    pub api_key: String,  // 敏感信息直接暴露
}

// ✅ 字段私有，通过受控方法访问
pub struct SessionConfig {
    db_path: PathBuf,
    api_key: SecretString,
}
impl SessionConfig {
    pub fn db_path(&self) -> &Path { &self.db_path }
    // api_key 不提供 getter——外部无需直接读取
}

// ❌ 胖 trait——机器人被迫实现 eat() 和 sleep()
pub trait Worker {
    fn work(&self) -> Result<()>;
    fn eat(&self, food: &Food) -> Result<()>;
    fn sleep(&self, hours: u8) -> Result<()>;
}

// ✅ 按能力拆分为小 trait——各实现者只实现自己需要的
pub trait Workable { fn work(&self) -> Result<()>; }
pub trait Eatable { fn eat(&self, food: &Food) -> Result<()>; }
pub trait Restable { fn sleep(&self, hours: u8) -> Result<()>; }
```

### TypeScript

```typescript
// ❌ 透传整个 domain 对象——Header 只需要 2 个字段却接收 15 个字段
interface HeaderProps { work: Work }
function Header({ work }: HeaderProps) {
  return <h1>{work.title} - {work.author}</h1>;
}

// ✅ Props 只声明实际需要的字段——依赖关系一目了然
interface HeaderProps { title: string; author: string }
function Header({ title, author }: HeaderProps) {
  return <h1>{title} - {author}</h1>;
}

// ❌ 巨型 hook 返回所有状态——调用方被迫依赖不需要的字段
function useSession() { return { messages, turnNumber, compactionStatus, apiConfig }; }

// ✅ 按关注点拆为独立 hook
function useMessages() { return { messages, setMessages }; }
function useTurn() { return { turnNumber, compactionStatus }; }
```

---

## 5. 开闭原则

**判断：** 新增功能时是否需要修改已有代码？需要则违反 OCP。扩展点通过 trait/注册机制/配置驱动打开。

### Rust

```rust
// ❌ 每新增一种工具就加一条 match 分支
fn dispatch_tool(name: &str, args: &Value) -> Result<ToolResult> {
    match name {
        "read_file" => read_file_tool(args),
        "write_file" => write_file_tool(args),
        "web_search" => web_search_tool(args),  // 每次新增都要改这里
        _ => Err(anyhow!("未知工具")),
    }
}

// ✅ trait + 注册机制——新增工具不改调度器
trait Tool: Send + Sync {
    fn name(&self) -> &str;
    fn execute(&self, args: &Value) -> Result<ToolResult>;
}
struct ToolRegistry { tools: HashMap<String, Box<dyn Tool>> }
impl ToolRegistry {
    fn register(&mut self, tool: Box<dyn Tool>) { self.tools.insert(tool.name().into(), tool); }
    fn dispatch(&self, name: &str, args: &Value) -> Result<ToolResult> {
        self.tools.get(name).ok_or_else(|| anyhow!("未知工具"))?.execute(args)
    }
}
```

### TypeScript

```typescript
// ❌ 新增状态就加 if 分支
function WorkStatusBadge({ status }: { status: string }) {
  if (status === 'running') return <Badge color="green">运行中</Badge>;
  if (status === 'paused') return <Badge color="yellow">已暂停</Badge>;
  // 新增 'archived' → 又要改这里
  return <Badge color="gray">未知</Badge>;
}

// ✅ 配置驱动——新增状态不改组件
const STATUS_MAP: Record<string, { color: string; label: string }> = {
  running: { color: 'green', label: '运行中' },
  paused: { color: 'yellow', label: '已暂停' },
};
function WorkStatusBadge({ status }: { status: string }) {
  const cfg = STATUS_MAP[status] ?? { color: 'gray', label: '未知' };
  return <Badge color={cfg.color}>{cfg.label}</Badge>;
}
```

---

## 6. 里氏替换原则

**判断：** 子类型替换基类型后，调用方能否在不知情的情况下正常工作？出现 `if (x instanceof SpecialCase)` 特判即违反 LSP。

### Rust

```rust
// ❌ trait 实现削弱父 trait 的契约
pub trait Cache {
    /// 此方法不执行任何 I/O 操作。  ← trait 声明的契约
    fn get(&self, key: &str) -> Option<String>;
}
struct DiskCache { db_path: PathBuf }
impl Cache for DiskCache {
    fn get(&self, key: &str) -> Option<String> {
        std::fs::read_to_string(self.db_path.join(key)).ok()  // 违反：实际执行了磁盘 I/O
    }
}

// ✅ 契约要诚实——要么不声明"无 I/O"，要么拆分 trait
pub trait Cache { fn get(&self, key: &str) -> Option<String>; }
pub trait AsyncCache { async fn get(&self, key: &str) -> Option<String>; }
```

### TypeScript

```typescript
// ❌ 组件替换时 ref 类型不兼容
const TextInput = forwardRef<HTMLInputElement, Props>((p, ref) => <input ref={ref} {...p} />);
const SearchInput = forwardRef<HTMLDivElement, Props>((p, ref) => <div ref={ref}>...</div>);
// SearchInput 试图替换 TextInput，但 ref 类型不同 → 破坏 LSP

// ✅ 统一接口类型，通过 useImperativeHandle 适配
interface FieldHandle { focus: () => void; blur: () => void; }
const TextInput = forwardRef<FieldHandle, Props>((p, ref) => {
  const inputRef = useRef<HTMLInputElement>(null);
  useImperativeHandle(ref, () => ({
    focus: () => inputRef.current?.focus(),
    blur: () => inputRef.current?.blur(),
  }));
  return <input ref={inputRef} {...p} />;
});
// TextInput 和 SearchInput 都实现 FieldHandle → 可互相替换
```

---

## 7. 依赖反转原则

**判断：** 高层模块是否直接 import 了低层模块的具体实现？是则违反 DIP。双方都应依赖抽象（trait/interface）。

### Rust

```rust
// ❌ 业务层直接依赖具体的 SQLite 实现
pub struct SessionManager {
    db: rusqlite::Connection,
}
impl SessionManager {
    pub fn save_message(&self, msg: &Message) -> Result<()> {
        self.db.execute("INSERT INTO messages ...", params![...])?;
        Ok(())
    }
}

// ✅ 依赖 trait——测试时可注入 MockStore
#[async_trait]
pub trait SessionStore: Send + Sync {
    async fn save_message(&self, msg: &Message) -> Result<()>;
}
pub struct SessionManager<S: SessionStore> { store: S }
// SessionManager 不依赖 rusqlite——任何实现 SessionStore 的类型都可注入
```

### TypeScript

```typescript
// ❌ 组件直接 import Tauri invoke——与 Rust 后端强耦合
import { invoke } from '@tauri-apps/api/core';
function WorkList() {
  const [works, setWorks] = useState<Work[]>([]);
  useEffect(() => { invoke<Work[]>('list_works').then(setWorks); }, []);
  return <ul>{works.map(w => <li key={w.name}>{w.name}</li>)}</ul>;
}

// ✅ 通过 hook 抽象数据源——组件只依赖 hook 的返回值类型
function useWorkList() {
  const [works, setWorks] = useState<Work[]>([]);
  useEffect(() => { invoke<Work[]>('list_works').then(setWorks); }, []);
  return { works };
}
function WorkList() {
  const { works } = useWorkList();
  return <ul>{works.map(w => <li key={w.name}>{w.name}</li>)}</ul>;
}
// 测试时可替换 useWorkList 的 mock，WorkList 无需改动
```

---

## 8. 清晰注释

**判断：** 注释解释**为什么**这样写，而非**做了什么**。代码通过命名表达 "做什么"——好的注释补充代码无法表达的信息。

```rust
// ❌ 废话注释 + 注释掉的旧代码
i += 1;  // 自增 i
// let result = old_function(data);  // 旧实现，暂时保留

// ✅ 解释非显而易见的决策
/// DeepSeek 对 Skill 摘要和 Tool 列表的顺序敏感——颠倒会导致 tool 调用遗漏。
pub fn build_system_prompt(skills: &[SkillSummary], tools: &[ToolDef]) -> String { ... }

// tokio::spawn 而非 spawn_blocking——
// rusqlite 的 Connection 在此版本实现了 Send 但不满足 spawn_blocking 的 'static 约束。
// TODO: rusqlite >= 0.32 后改回 spawn_blocking。
tokio::spawn(async move { db_operation().await })
```

```typescript
// ❌ 复述代码
if (loading) return <Spinner />;  // 如果正在加载，显示 Spinner

// ✅ 解释为什么 + 潜在陷阱
/**
 * 订阅当前活动作品的实时状态变更（Tauri IPC 事件）。
 * @sideeffects 挂载时订阅事件；workName 变化时重建订阅；卸载时取消。
 */
function useWorkStatus(workName: string): WorkStatusState { ... }

// 空依赖数组：此 effect 仅需挂载时初始化一次 WebSocket，
// 后续重连由 WebSocket 自身的 onclose 回调处理。
useEffect(() => { connectWebSocket(url); }, []);
```

---

## 重构执行流程

改动代码时按以下步骤执行——**每一步通过才进入下一步**，修改代码后回到步骤 1 重跑。

### 1. 确认范围（Preserve Functionality）

- 通读待改代码的**全部调用方**，确认每个调用方的使用方式
- 明确 "改动前这段代码做了什么"——用一句话描述当前行为
- 如果有测试：先跑一遍确认基线绿。如果没有：补一个最小测试覆盖当前行为

### 2. 应用原则（Apply Standards）

逐条对照上方的 8 个原则做判断：

| 优先级 | 原则 | 审查切入点 |
|--------|------|------------|
| 1 | 单一职责 | 这个模块是否只做一件事？ |
| 2 | DRY | 是否存在同一知识的重复？ |
| 3 | 最小暴露 | 新增的 pub/item 外部真的需要吗？ |
| 4 | 正交 + 依赖反转 | 改动是否波及无关模块？ |
| 5 | 开闭 + 里氏替换 | 新增功能是否需改已有代码？子类型能否无缝替换？ |
| 6 | 清晰注释 | 不能一眼看出的意图是否有注释？ |

### 3. 简化结构（Enhance Clarity）

- 消除嵌套三元：多条件用 `match` / `switch` / `if-else`，不嵌套 `?:`
- 显式优于紧凑：不因"少写几行"而牺牲可读性。单独的变量声明 > 内联的长表达式
- 消除死代码：注释掉的旧代码用 git 管理，不留在源码中
- 合并过度拆分：如果一个 3 行的函数只被调一次且语义不独立，内联回去

### 4. 自检（Verify）

- 逐行 diff：每一处修改都能说清**为什么改**和**行为是否不变**
- 重跑步骤 1 的测试：确认基线仍绿
- 如果有新增逻辑：补测试覆盖新路径

---

## 重构检查清单

改动完成后逐条确认：

- [ ] **功能不变**：所有原有行为、输出、副作用保持不变。新增功能不影响已有调用方
- [ ] **职责单一**：改动的每个函数/组件只做一件事。"和"字连接的描述应拆分
- [ ] **无重复**：没有复制粘贴的代码块、重复的错误类型、相同的逻辑散落两处
- [ ] **暴露最小**：新增的 `pub` / `export` 每个都有被外部需要的理由
- [ ] **不改已有代码**（OCP）：新增功能通过扩展（新 trait 实现/新组件/配置项），不修改已有 match/if-else 分支
- [ ] **依赖倒置**：业务逻辑不直接 import 具体实现（`rusqlite::Connection`、`invoke('cmd')`）；通过 trait/hook 抽象
- [ ] **无嵌套三元**：多条件判断使用 `match`/`switch`/`if-else`，不嵌套 `?:`
- [ ] **无死代码**：没有注释掉的旧代码、从未调用的函数、仅测试用的 `pub` 导出
- [ ] **注释恰当**：解释 "为什么"，不解释 "做了什么"；过时注释已更新或删除
- [ ] **测试覆盖**：改动路径有测试；新增逻辑有测试；重构后原测试仍绿

---

## 冲突裁决

当原则冲突时按以下顺序取舍：

1. **单一职责 > DRY**：错误地合并两个职责不同但文本相似的代码，比保留两段清晰独立的代码危害更大
2. **功能不变 > 简化**：不能为了让代码 "更好看" 而改变行为
3. **显式 > 紧凑**：多写几行 > 一行塞进所有逻辑
