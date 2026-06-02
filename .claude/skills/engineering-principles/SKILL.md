---
name: engineering-principles
description: >-
  重构或新增 feature（Rust / TypeScript）时遵循的软件工程基本原则：
  DRY（不要重复自己）、单一职责与高内聚、正交、最小暴露与接口隔离、
  开闭原则、里氏替换原则、依赖反转原则、清晰注释。
  用于架构决策、代码拆分、接口设计、抽象边界审查。
---

# 软件工程基本原则

在本项目内进行重构或新增 feature，涉及 **Rust**（crates/）或 **TypeScript**（ui/）
代码时，必须在设计阶段和代码审查阶段遵循以下原则。这些原则共同指向**可维护、可扩展、易理解**
的软件——不要求教条式遵守每一项的极端形式，但每个设计决策都应有意识地考量它们。

---

## 1. DRY —— Don't Repeat Yourself（不要重复自己）

**核心思想**：每一份知识、逻辑或规则在系统中应当只有唯一、明确的表示。

违反 DRY 的代价是，当需求变化时，你需要找到所有重复点逐一修改，遗漏任何一处都会产生不一致的缺陷。
DRY 消灭的是**同一知识**的重复，不是**巧合相似**的文本：如果两段代码服务于不同业务语义、
有不同的变化节奏，强行合并反而创造了耦合，比保留两份独立代码更糟糕。

**要求**：
- Rust：相同校验、转换逻辑提取为 crate 内公共函数；跨 crate 通用工具放入 `novel-tools`；
  相似错误类型统一用 `thiserror` enum 变体表达，避免多 crate 各自定义语义相同的错误。
- TS/React：重复的 `useState + useEffect` 组合抽取为自定义 hook；跨组件类型定义集中到
  `ui/src/types/`；API 调用封装到统一 service 层，禁止散落相同的 `invoke` 调用。

**Rust 示例**：
```rust
// 反例：两个 crate 各自定义语义相同的错误
// crates/novel-core/src/error.rs
pub enum CoreError {
    WorkNotFound(String),
    // ...
}

// crates/novel-server/src/error.rs
pub enum ServerError {
    WorkNotFound(String),  // 与 CoreError 语义完全相同，重复定义
    // ...
}

// 正例：公共错误类型统一定义，各 crate 通过 thiserror 的 #[from] 组合
// crates/novel-tools/src/error.rs
#[derive(Debug, thiserror::Error)]
pub enum AgentError {
    #[error("作品不存在: {0}")]
    WorkNotFound(String),
    // ...
}

// crates/novel-core/src/error.rs
#[derive(Debug, thiserror::Error)]
pub enum CoreError {
    #[error(transparent)]
    Agent(#[from] AgentError),
    // Core 独有的错误变体
}
```

**TypeScript / React 示例**：
```typescript
// 反例：两个组件重复相同的 listen 逻辑
function ChatPanel() {
  const [msgs, setMsgs] = useState<Message[]>([]);
  useEffect(() => {
    const unlisten = listen<MsgEvent>('new-msg', e => setMsgs(p => [...p, e.payload]));
    return () => { unlisten.then(f => f()); };
  }, []);
  // ...
}
function NotificationBar() {
  const [msgs, setMsgs] = useState<Message[]>([]);
  useEffect(() => {
    const unlisten = listen<MsgEvent>('new-msg', e => setMsgs(p => [...p, e.payload]));
    return () => { unlisten.then(f => f()); };
  }, []);
  // ...
}

// 正例：抽取为自定义 hook
function useIncomingMessages() {
  const [msgs, setMsgs] = useState<Message[]>([]);
  useEffect(() => {
    const unlisten = listen<MsgEvent>('new-msg', e => setMsgs(p => [...p, e.payload]));
    return () => { unlisten.then(f => f()); };
  }, []);
  return msgs;
}

function ChatPanel() {
  const msgs = useIncomingMessages();
  // ...
}
```

---

## 2. 单一职责与高内聚（Single Responsibility & High Cohesion）

这两个原则从不同角度描述同一个目标：**一个模块应该只做一件事**。

**单一职责（SRP，SOLID-S）** 一个类（struct / 模块 / 函数）应有且仅有
一个引起变化的原因。"原因"指的是**角色或职责**——如果某个 struct 同时服务于两个不同的角色
（如既负责 LLM 调用又负责数据库写入），任何一个角色的需求变化都可能导致该 struct 被修改，
增加回归风险。

**高内聚** 衡量模块内部元素彼此关联的紧密程度。高内聚的模块，其所有方法都操作同一组数据、
服务于同一个目标；低内聚的模块内部方法之间互不相关，修改一处极易影响其他无关方法。

**两者关系**：单一职责是高内聚的成因——只做一件事的模块，内部元素必然紧密相关。反过来，
高内聚是单一职责在模块内部的可观察结果——当你发现一个模块内部的方法都围绕同一个目标时，
它自然也就满足了单一职责。

**要求**：
- Rust：struct 的 `impl` 方法超过 10 个且分组不相关时拆分；函数不做"解析→调 API→写库→
  发事件"的串联（`parse_and_save` 应拆为两个函数）；`EngineState` 只管引擎状态，
  不混入持久化或 LLM 调用逻辑。
- TS/React：组件分离为展示型（只渲染 UI）和容器型（管理数据）；一个 `useEffect` 只处理
  一个副作用关注点；`lib.rs`/`mod.rs` 超 300 行或 `.ts` 文件导出超 10 个不相关符号时
  拆分子模块。

**Rust 示例**：
```rust
// 反例：一个函数串联多层职责
fn handle_user_message(msg: &str, db: &SessionDb) -> Result<Response> {
    let parsed = parse_message(msg)?;            // 解析
    let llm_resp = call_deepseek(&parsed)?;      // 调用 LLM
    db.save_turn(&parsed, &llm_resp)?;           // 持久化
    emit_event("turn-complete", &llm_resp)?;     // 发送事件
    Ok(llm_resp)
}

// 正例：每层独立，由上层编排
fn parse_and_call(msg: &str) -> Result<(ParsedMessage, LlmResponse)> {
    let parsed = parse_message(msg)?;
    let llm_resp = call_deepseek(&parsed)?;
    Ok((parsed, llm_resp))
}

fn persist_and_emit(parsed: &ParsedMessage, resp: &LlmResponse, db: &SessionDb) -> Result<()> {
    db.save_turn(parsed, resp)?;
    emit_event("turn-complete", resp)?;
    Ok(())
}
```

**TypeScript / React 示例**：
```typescript
// 反例：一个组件管数据加载、事件监听、UI 渲染
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

// 正例：逻辑抽取到 hook，组件只渲染
function useWorkDetail(name: string) { /* 数据加载 */ return { detail, loading, error }; }
function useWorkStatus(name: string) { /* 事件监听 */ return status; }
function WorkDetail({ name }: { name: string }) {
  const { detail, loading } = useWorkDetail(name);
  const status = useWorkStatus(name);
  if (loading) return <Spinner />;
  return <div><StatusBadge status={status} /><h1>{detail.title}</h1></div>;
}
```

---

## 3. 正交（Orthogonality）

**核心思想**：模块之间的变化互相独立——修改一处不会在不相关的功能中引发副作用。

正交系统最直观的好处是消除涟漪效应。改一行代码不需要连锁修改十个文件。它也是团队并行开发的基石：两个开发者
各自修改两个正交的模块，不会产生合并冲突。反之，低正交的系统每次集成都是一场灾难。

**在这一项目中**：
- `novel-core` 与 `novel-server` 通过 `EngineCommand`/`Event` 枚举交互，双方可独立演化。
- 前端 UI 与后端通过 Tauri `invoke` 通信，IPC 签名一致即可独立开发。
- `novel-knowledge` 与 `novel-state` 通过 trait 边界协作，互不侵入。

**要求**：
- Rust：crate 之间通过明确的 trait 边界和枚举类型交互，避免跨 crate 直接访问内部 struct 字段；
  数据库 schema 变更不影响 LLM 调用逻辑。
- TS/React：通用 UI 组件（Button、Modal）不引入 Tauri `invoke` 或业务类型，通过回调让业务
  页面桥接；每个 React Context 只负责一个独立关注点；路由通过配置式映射驱动，新增页面不修改
  路由框架。

**检查方法**："修改模块 A 是否需要同时修改 B、C、D？"需要则不正交。

**Rust 示例**：
```rust
// 反例：novel-server 直接访问 novel-core 的内部字段
// crates/novel-server/src/engine_loop.rs
fn build_response(engine: &EngineState) -> String {
    // 直接读取 core 的内部数据结构——core 改字段名，这里就炸
    format!("turn: {}, messages: {}", engine.inner.turn_count, engine.inner.messages.len())
}

// 正例：通过公开查询方法访问，core 内部变化不影响 server
// crates/novel-core/src/engine.rs
impl EngineState {
    pub fn turn_count(&self) -> usize { self.inner.turn_count }
    pub fn message_count(&self) -> usize { self.inner.messages.len() }
}

// crates/novel-server/src/engine_loop.rs
fn build_response(engine: &EngineState) -> String {
    format!("turn: {}, messages: {}", engine.turn_count(), engine.message_count())
}
```

**TypeScript / React 示例**：
```typescript
// 反例：UI 组件直接耦合 Tauri IPC——无法在纯浏览器环境测试
function SaveButton({ name }: { name: string }) {
  return <button onClick={() => invoke('save', { name })}>Save</button>;
}

// 正例：通过回调解耦——Button 是纯 UI，业务逻辑由父级注入
function SaveButton({ onSave }: { onSave: () => void }) {
  return <button onClick={onSave}>Save</button>;
}
function WorkPage({ name }: { name: string }) {
  return <SaveButton onSave={() => invoke('save', { name })} />;
}
```

> 实现正交的关键技术见第 7 节「依赖反转原则」。

---

## 4. 最小暴露与接口隔离（Least Exposure & Interface Segregation）

这两个原则分别从**提供方**和**消费方**两个角度限制接口的可见范围，本质上互为补充。

### 4.1 最小暴露（提供方视角）

一个模块只应暴露那些必须让外部知道的 public API，内部实现细节和辅助方法全部保持私有。对外部调用方而言，"知道的越少越好"。
当内部实现变化时（优化算法、替换数据结构、修复 bug），只要暴露的接口不变，所有调用方都无需改动。
如果暴露过多内部细节，调用方迟早会依赖这些细节，最终任何内部改动都变成破坏性变更。

**要求**：
- Rust：struct 字段默认私有，通过公开方法访问；仅在 `lib.rs` 中 `pub mod` 对外模块；
  不为"将来可能用到"而提前公开接口——需要时再加 `pub` 的成本极低；
  `Cargo.toml` 不开启不必要 feature。
- TS/React：模块 `index.ts` 仅 re-export 对外承诺的 API；组件 Props 只声明实际需要的
  字段——不为 3 个字段透传包含 20 个字段的 domain 对象；自定义 hook 只返回调用方需要的
  状态和操作；`package.json` 不暴露内部模块路径。

**Rust 示例**：
```rust
// 反例：struct 字段全部公开，外部可以任意修改内部状态
pub struct SessionConfig {
    pub db_path: PathBuf,
    pub max_turns: usize,
    pub model: String,
    pub api_key: String,  // 敏感信息直接暴露
}

// 正例：字段私有，通过受控方法访问，敏感信息不暴露
pub struct SessionConfig {
    db_path: PathBuf,
    max_turns: usize,
    model: String,
    api_key: SecretString,  // 敏感信息用封装类型
}

impl SessionConfig {
    pub fn db_path(&self) -> &Path { &self.db_path }
    pub fn max_turns(&self) -> usize { self.max_turns }
    pub fn model(&self) -> &str { &self.model }
    // api_key 不提供 getter——外部无需直接读取
}
```

**TypeScript / React 示例**：
```typescript
// 反例：透传整个 domain 对象——Header 只需要 2 个字段却接收 15 个字段的 Work
interface WorkHeaderProps { work: Work }
function Header({ work }: WorkHeaderProps) {
  return <h1>{work.title} - {work.author}</h1>;
}

// 正例：Props 只声明实际需要的字段
interface WorkHeaderProps { title: string; author: string }
function Header({ title, author }: WorkHeaderProps) {
  return <h1>{title} - {author}</h1>;
}
// 调用侧显式解构，依赖关系一目了然
<Header title={work.title} author={work.author} />
```

### 4.2 接口隔离 / ISP（消费方视角，SOLID-I）

**核心思想**：不应该强迫一个类型实现它用不到的方法，也不应强迫模块依赖它不需要的接口。

如果一个 trait / interface 过于臃肿，它的每个实现者都必须处理与自己无关的方法（填 `todo!()`、
抛 `NotImplementedError`、或空实现），而调用方也被迫"看到"这些他们并不关心的方法签名。
将"胖接口"拆分为多个小而专注的接口后，每个实现者只实现自己真正需要的方法，调用方也只
依赖自己真正使用的接口——修改某个小接口不会波及使用其他接口的代码。

**要求**：
- Rust：将"胖 trait"拆分为多个小 trait（如 `Workable` + `Eatable` 而非 `Worker` 包含两者）；
  模块只 `use` 实际调用的 trait 和类型。
- TS/React：大 interface 拆分为小 interface（`Loadable<T>`、`Selectable<T>`）；自定义 hook
  按关注点拆为独立 hook，不返回巨型对象。

**Rust 示例**：
```rust
// 反例：胖 trait——机器人被迫实现它不需要的 eat()
pub trait Worker {
    fn work(&self) -> Result<()>;
    fn eat(&self, food: &Food) -> Result<()>;   // 机器人不需要
    fn sleep(&self, hours: u8) -> Result<()>;   // 机器人不需要
}

// 正例：按能力拆分为小 trait
pub trait Workable { fn work(&self) -> Result<()>; }
pub trait Eatable { fn eat(&self, food: &Food) -> Result<()>; }
pub trait Restable { fn sleep(&self, hours: u8) -> Result<()>; }

// 机器人只实现 Workable；人类实现 Workable + Eatable + Restable
impl Workable for Robot { fn work(&self) -> Result<()> { /* ... */ } }
```

**TypeScript / React 示例**：
```typescript
// 反例：巨型 hook 返回所有状态，调用方被迫依赖不需要的字段
function useSession() { return { messages, turnNumber, compactionStatus, apiConfig, ... }; }

// 正例：按关注点拆分为独立 hook
function useMessages() { return { messages, setMessages }; }
function useTurn() { return { turnNumber, compactionStatus }; }
function useApiConfig() { return { apiConfig, setApiConfig }; }
// 调用方只使用自己需要的 hook
```

---

## 5. 开闭原则（Open/Closed Principle, OCP，SOLID-O）

**核心思想**：软件实体（struct、trait、模块、组件）应当对扩展开放，对修改关闭。

Bertrand Meyer 在 1988 年提出这一原则时，核心洞察是：**每一次修改已有代码都有可能引入回归**。
当系统稳定运行后，新增功能不应通过"在已有函数里加 if 分支"来实现，而应通过新增代码、
利用已有的抽象扩展点来实现。这样，旧行为完全不受影响，新行为被干净地隔离在新的代码单元中。

**要求**：
- Rust：通过 trait 抽象和多态，新增 `Tool` 实现不改调度器核心逻辑；`TerminalReason` 新增变体
  时评估是否不改匹配逻辑（或通过 `#[non_exhaustive]` 预留扩展空间）。
- TS/React：组件通过 `children`/render props 开放扩展而非常见条件分支；使用泛型组件适配多种
  数据类型；新增页面不应修改路由框架——路由通过 `{ path, component }[]` 配置驱动。

**Rust 示例**：
```rust
// 反例：每新增一种工具就要在调度器 match 中加一条分支
fn dispatch_tool(name: &str, args: &Value) -> Result<ToolResult> {
    match name {
        "read_file" => read_file_tool(args),
        "write_file" => write_file_tool(args),
        // 每次新增工具都要改这里 → 违反 OCP
        "web_search" => web_search_tool(args),
        _ => Err(anyhow!("未知工具: {name}")),
    }
}

// 正例：通过 trait + 注册机制，新增工具不改调度器
trait Tool: Send + Sync {
    fn name(&self) -> &str;
    fn execute(&self, args: &Value) -> Result<ToolResult>;
}

struct ToolRegistry { tools: HashMap<String, Box<dyn Tool>> }

impl ToolRegistry {
    fn register(&mut self, tool: Box<dyn Tool>) {
        self.tools.insert(tool.name().to_string(), tool);
    }
    fn dispatch(&self, name: &str, args: &Value) -> Result<ToolResult> {
        self.tools.get(name).ok_or_else(|| anyhow!("未知工具: {name}"))?.execute(args)
    }
}
```

**TypeScript / React 示例**：
```typescript
// 反例：每次新增作品状态就在组件内加条件分支
function WorkStatusBadge({ status }: { status: string }) {
  if (status === 'running') return <Badge color="green">运行中</Badge>;
  if (status === 'paused') return <Badge color="yellow">已暂停</Badge>;
  if (status === 'error') return <Badge color="red">错误</Badge>;
  // 新增 'archived' 状态 → 又要改这里
  return <Badge color="gray">未知</Badge>;
}

// 正例：状态映射配置驱动，新增状态不改组件
const STATUS_MAP: Record<string, { color: string; label: string }> = {
  running: { color: 'green', label: '运行中' },
  paused: { color: 'yellow', label: '已暂停' },
  error: { color: 'red', label: '错误' },
};
function WorkStatusBadge({ status }: { status: string }) {
  const cfg = STATUS_MAP[status] ?? { color: 'gray', label: '未知' };
  return <Badge color={cfg.color}>{cfg.label}</Badge>;
}

// 正例 2：通过 children 开放扩展，而非硬编码内部结构
function Card({ title, children }: { title: string; children: React.ReactNode }) {
  return <div className="card"><h2>{title}</h2><div>{children}</div></div>;
}
```

---

## 6. 里氏替换原则（Liskov Substitution Principle, LSP，SOLID-L）

**核心思想**：子类型必须能完全替换基类型而不破坏程序正确性。

如果对每个类型 T 的对象 o1，都存在类型 S 的对象 o2，使得对针对 T 定义的所有程序 P，当用 o2 替换 o1 时 P 的行为不变，则 S 是 T 的子类型。
通俗地说：**调用方应该能在不知情的情况下使用任何子类型**——不需要`if (x instanceof SpecialCase)` 这样的特判。
违反 LSP 的最常见信号就是调用方代码中出现了针对具体子类型的特判分支。

**要求**：
- Rust：trait 文档（`///`）必须声明实现者需遵守的契约（前置条件、后置条件、不变量）；
  实现不可削弱 trait 约定的语义（如父 trait 声明"此方法永不 panic"，实现不可引入 panic）；
  不应在调用侧对具体实现类型做 downcast 判断。
- TS/React：多态组件（`as` prop）替换后需保持相同行为契约和无障碍属性；
  `forwardRef` 替换时 ref 类型必须兼容；父组件通过 slots 传入的子组件必须接受父级承诺传入
  的 props。

**Rust 示例**：
```rust
// 反例：trait 实现削弱了父 trait 的契约
pub trait Cache {
    /// 从缓存中获取值，键不存在时返回 None。
    /// 此方法不执行任何 I/O 操作。  ← trait 声明的契约
    fn get(&self, key: &str) -> Option<String>;
}

struct DiskCache { db_path: PathBuf }

impl Cache for DiskCache {
    fn get(&self, key: &str) -> Option<String> {
        // 违反契约：实际上执行了磁盘 I/O
        std::fs::read_to_string(self.db_path.join(key)).ok()
    }
}

// 正例：要么不声明"无 I/O"的契约，要么单独定义同步/异步 trait
pub trait Cache {
    /// 从缓存中获取值，键不存在时返回 None。
    fn get(&self, key: &str) -> Option<String>;
}

pub trait AsyncCache {
    async fn get(&self, key: &str) -> Option<String>;
}
```

**TypeScript / React 示例**：
```typescript
// 反例：组件替换时 ref 类型不兼容
// TextInput 暴露 HTMLInputElement ref
const TextInput = forwardRef<HTMLInputElement, Props>((p, ref) => <input ref={ref} {...p} />);
// SearchInput 试图替换 TextInput，但暴露了 HTMLDivElement ref → 破坏 LSP
const SearchInput = forwardRef<HTMLDivElement, Props>((p, ref) => <div ref={ref}>...</div>);

// 正例：定义统一的接口类型，通过 useImperativeHandle 适配
interface FieldHandle { focus: () => void; blur: () => void; }

const TextInput = forwardRef<FieldHandle, Props>((p, ref) => {
  const inputRef = useRef<HTMLInputElement>(null);
  useImperativeHandle(ref, () => ({
    focus: () => inputRef.current?.focus(),
    blur: () => inputRef.current?.blur(),
  }));
  return <input ref={inputRef} {...p} />;
});

const SearchInput = forwardRef<FieldHandle, Props>((p, ref) => {
  const divRef = useRef<HTMLDivElement>(null);
  useImperativeHandle(ref, () => ({
    focus: () => (divRef.current?.querySelector('input') as HTMLElement)?.focus(),
    blur: () => (divRef.current?.querySelector('input') as HTMLElement)?.blur(),
  }));
  return <div ref={divRef}><input {...p} /></div>;
});
// TextInput 和 SearchInput 都实现 FieldHandle → 可互相替换
```

---

## 7. 依赖反转原则（Dependency Inversion Principle, DIP，SOLID-D）

**核心思想**：高层模块和低层模块都应该依赖抽象，而不是高层直接依赖低层。

传统分层架构中依赖方向与控制流方向一致：业务层调用数据层，业务层就 import 数据层的具体类。
这导致更换数据库时业务层必须修改——因为业务层直接依赖了数据库的具体实现。DIP 将这一关系
**反转**：业务层定义 `Repository` trait（抽象），数据层**实现**该 trait。控制流仍然是
业务层→数据层，但源码依赖方向变为数据层→业务层（因为数据层要 import 业务层定义的 trait）。

DIP 是实现**正交性**的关键技术——只有当双方都依赖抽象时，才能各自独立变化。

**要求**：
- Rust：业务层（`novel-core`）依赖 `Tool` trait / `SessionStore` trait，不依赖具体实现；
  通过泛型参数或 `Arc<dyn Trait>` 注入具体实现。
- TS/React：
  - Context 作为 DI 容器：将服务实例通过 Context 注入，组件只依赖接口。
  - 自定义 hook 作为抽象层：组件不直接 `invoke('cmd')`，通过 hook 间接获取数据。

**Rust 示例**：
```rust
// 反例：业务层直接依赖具体的 SQLite 实现
pub struct SessionManager {
    db: rusqlite::Connection,  // 直接依赖具体数据库
}

impl SessionManager {
    pub fn save_message(&self, msg: &Message) -> Result<()> {
        self.db.execute("INSERT INTO messages ...", params![...])?;
        Ok(())
    }
}

// 正例：业务层依赖 trait，具体实现通过泛型注入
#[async_trait]
pub trait SessionStore: Send + Sync {
    async fn save_message(&self, msg: &Message) -> Result<()>;
    async fn load_messages(&self, session_id: &str) -> Result<Vec<Message>>;
}

pub struct SessionManager<S: SessionStore> {
    store: S,
}
// 测试时注入 MockStore，生产环境注入 SqliteStore，SessionManager 无需改动
```

**TypeScript / React 示例**：
```typescript
// 反例：组件直接 import Tauri invoke——与 Rust 后端强耦合
import { invoke } from '@tauri-apps/api/core';
function WorkList() {
  const [works, setWorks] = useState<Work[]>([]);
  useEffect(() => { invoke<Work[]>('list_works').then(setWorks); }, []);
  return <ul>{works.map(w => <li key={w.name}>{w.name}</li>)}</ul>;
}

// 正例：通过 hook 抽象数据源——组件只依赖 hook 的返回值类型
function useWorkList() {
  const [works, setWorks] = useState<Work[]>([]);
  useEffect(() => { invoke<Work[]>('list_works').then(setWorks); }, []);
  return { works };
}
function WorkList() {
  const { works } = useWorkList();
  return <ul>{works.map(w => <li key={w.name}>{w.name}</li>)}</ul>;
}
// 测试时可替换 useWorkList 的 mock 实现，WorkList 组件无需改动
```

---

## 8. 清晰注释（Clear Comments）

**核心思想**：注释用来解释**为什么**这样实现，而不是**做了什么**。

代码本身通过命名和结构表达"做什么"和"怎么做"——一个好的函数名已经说明了它的行为，
清晰的变量名已经表明了数据的含义。注释的价值在于补充代码**无法**表达的东西：为什么选
这个算法而不是另一个？这个看起来奇怪的判断条件背后有什么业务规则？这个 workaround
是为了绕过哪个框架 bug，何时可以移除？当未来的维护者（可能是你自己）面对一段代码
百思不得其解时，好的注释就是关键时刻的救命稻草。反之，过时或错误的注释比没有注释
更糟糕——它会主动误导维护者。

**推荐注释内容**：非显而易见的业务规则/公式来源、临时 workaround 及移除条件、
公共 API 文档、复杂算法意图或参考链接、不显而易见的性能考量。

**应避免**：复述代码的废话（`i++ // 自增 i`）、注释掉的旧代码（用 git）、
错误或过时的注释、用注释为糟糕命名打补丁。

**Rust**：
```rust
// 反例：废话注释 + 注释掉的旧代码
// 自增 i
i += 1;

// 旧实现：let result = old_function(data);
// 暂时保留以备回退
let result = new_function(data);

// 正例：解释非显而易见的决策
/// 构建 system prompt。注意：Skill 摘要必须放在元数据之后、
/// Tool 列表之前——DeepSeek 模型对该顺序敏感，颠倒会导致 tool 调用遗漏。
pub fn build_system_prompt(skills: &[SkillSummary], tools: &[ToolDef]) -> String {
    // ...
}

// workaround：tokio::spawn 而非 spawn_blocking——
// rusqlite 的 Connection 在该版本中实现了 Send 但不支持 spawn_blocking 的 'static 约束。
// TODO: rusqlite >= 0.32 后改回 spawn_blocking。参见 https://github.com/.../issues/123
tokio::spawn(async move { db_operation().await })
```

**TypeScript / React**：
```typescript
// 反例：注释与代码完全一致
// 如果正在加载，显示 Spinner
if (loading) return <Spinner />;

// 正例：解释为什么和潜在陷阱
/**
 * 订阅当前活动作品的实时状态变更（通过 Tauri IPC 事件）。
 *
 * @param workName - 作品名称，传入空字符串时不订阅。
 * @returns { status, turnNumber, lastError }
 * @sideeffects 挂载时订阅 Tauri 事件；workName 变化时重建订阅；卸载时取消。
 */
function useWorkStatus(workName: string): WorkStatusState { ... }

// 空依赖数组的原因：此 effect 仅需在组件挂载时初始化一次 WebSocket 连接，
// 后续的 reconnection 由 WebSocket 自身的 onclose 回调处理。
useEffect(() => { connectWebSocket(url); }, []);
```

---

## 总结与关联

这些原则相互交织，在设计和审查时按以下优先级依次考虑：

| 优先级 | 原则 | 审查切入点 |
|--------|------|------------|
| 1 | 单一职责与高内聚 | 这个模块是否只做一件事？ |
| 2 | DRY | 是否存在同一知识的重复表达？ |
| 3 | 最小暴露与接口隔离 | 暴露的 API 是否过大？调用方是否被迫知道太多？ |
| 4 | 正交 + 依赖反转 | 修改是否波及无关模块？依赖是否指向抽象？ |
| 5 | 开闭 + 里氏替换 | 新增功能是否需修改已有代码？子类型能否无缝替换？ |
| 6 | 清晰注释 | 不能一眼看出的意图是否有注释说明？ |

当原则冲突时（如 DRY vs 单一职责），优先保证**单一职责**和**高内聚**——错误地合并两个
职责不同但文本相似的代码，比保留两段清晰独立的代码危害更大。
