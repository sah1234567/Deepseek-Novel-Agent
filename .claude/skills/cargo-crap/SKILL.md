---
name: cargo-crap
description: >-
  用 cargo-crap 计算 Rust 函数的 CRAP（Change Risk Anti-Patterns）分数：结合圈复杂度与测试覆盖率，
  找出「复杂且未充分测试」的高风险函数。覆盖 lcov 生成（cargo llvm-cov + nextest）、
  `cargo crap --lcov lcov.info` 分析、阈值/排除项配置（`.cargo-crap.toml`）、
  多格式输出（human/json/github/markdown/pr-comment/sarif）、CI 门禁与回归基线。
  在用户提到 CRAP、cargo crap、lcov.info、覆盖率质量门禁、复杂度过高函数时使用。
---

# cargo-crap：CRAP 复杂度 × 覆盖率分析

## 是什么

`cargo crap`（crate v0.2.2）将**圈复杂度（CC）**和**行覆盖率**合成 CRAP 风险分数：

```text
CRAP(m) = comp(m)² × (1 − cov(m)/100)³ + comp(m)
```

- CC=1 且 100% 覆盖 → CRAP = 1.0（下界）
- 100% 覆盖时 CRAP = CC（两者相等是正常现象）
- CC ≳ 30 → 无论覆盖率多高都难以低于默认阈值 30

源码：[minikin/cargo-crap](https://github.com/minikin/cargo-crap) · [docs.rs](https://docs.rs/cargo-crap/latest/cargo_crap/)

## 标准流程

**一键（推荐）**：`bash scripts/ci-crap.sh` — 同一 shell 内先 `llvm-cov nextest` 再 `cargo crap --fail-above`（见 `scripts/README.md`）。

### 1. 生成 LCOV 覆盖率

```bash
# Linux/macOS（workspace + nextest + CI profile）
cargo llvm-cov nextest --workspace --all-features --lcov --output-path lcov.info -- --workspace --profile ci
```

```powershell
# Windows（nextest 不接受 `--` 后的 `--profile ci`，用环境变量）
$env:NEXTEST_PROFILE = "ci"
cargo llvm-cov nextest --workspace --all-features --lcov --output-path lcov.info
```

单 crate：`cargo llvm-cov --lcov --output-path lcov.info`，然后 `cargo crap --lcov lcov.info --path crates/<name> --min 30`。

> **硬性要求：** Rust 测试与覆盖率**只用** `cargo nextest run` / `cargo llvm-cov nextest`，**禁止** `cargo test`。`lcov.info` 已在 `.gitignore`；不要把 `run` 写在 `nextest` 与 `--` 之间。
>
> **重构后必须重生 lcov**：目录移动（如 `turn/loop/`、`db/` 拆分）会使旧 `lcov.info` 路径对不上，覆盖率按 0% 计，CRAP 会虚高（曾出现 68 个假阳性，重生后降至 0）。

### 2. 运行分析

```bash
# 只显示高风险函数（推荐）
cargo crap --lcov lcov.info --workspace --min 30

# 硬门禁（CI 用）
cargo crap --lcov lcov.info --workspace --fail-above --threshold 30
```

### 3. 关键标志（决策相关，其余用 `--help` 查）

| 标志 | 作用 |
|------|------|
| `--threshold <N>` | 超标分数线，默认 30 |
| `--min <N>` | 只显示 CRAP ≥ N 的函数（过滤噪音） |
| `--fail-above` | 超标时 exit 1（CI 门禁） |
| `--baseline <json>` | 与上次报告对比，检测回归 |
| `--fail-regression` | 相对 baseline 分数上升则 exit 1 |
| `--format <fmt>` | `human`（终端）/ `json`（baseline存档）/ `github`（Actions 注解）/ `pr-comment`（PR 评论）/ `sarif`（Code Scanning） |

## 超标函数的修复方法

CRAP 高的根因是**复杂度高**和/或**测试少**。按以下优先级处理：

### 先判断：复杂度驱动 vs 覆盖率驱动

| CRAP 高，CC 低 | CRAP 中等，CC 高 | CRAP 高，CC 也高 |
|---------------|-----------------|-----------------|
| 缺测试。补测试即可 | 函数太复杂。先重构拆分 | 两者都严重。先拆再补 |

### 常见重构模式

**1. 提取条件分支（最常见的高 CC 来源）**

```rust
// Before: 多层嵌套 match/if → CC=12
fn handle_command(cmd: &Command, db: &Db, cache: &Cache) -> Result<Response> {
    match cmd.kind {
        Kind::Read => {
            if let Some(cached) = cache.get(&cmd.key) {
                // ... 5 行处理逻辑
            } else {
                let val = db.query(&cmd.key)?;
                // ... 5 行处理逻辑
            }
        }
        Kind::Write => { /* ... */ }
        Kind::Delete => { /* ... */ }
    }
}

// After: 每种分支提取为独立函数 → CC≤3
fn handle_command(cmd: &Command, db: &Db, cache: &Cache) -> Result<Response> {
    match cmd.kind {
        Kind::Read => handle_read(cmd, db, cache),
        Kind::Write => handle_write(cmd, db),
        Kind::Delete => handle_delete(cmd, db),
    }
}
```

**2. 提取验证/转换逻辑（match+if 嵌套的常见来源）**

```rust
// Before: 在一个函数里 parse + validate + process
fn process(input: &str) -> Result<Output> {
    let parsed = input.parse::<Config>()?;       // 可能 panic 的分支
    if parsed.timeout > 300 { return Err(...); }  // 验证分支
    if parsed.retries == 0 { /* 默认值逻辑 */ }
    // ... 核心处理逻辑
}

// After: 拆为 parse_config + validate + execute
fn process(input: &str) -> Result<Output> {
    let config = parse_and_validate(input)?;
    execute(config)
}
```

**3. 替换重复的条件链为查表**

```rust
// Before: 长 if-else 链 → CC=7
fn status_label(code: u8) -> &'static str {
    if code == 0 { "ok" }
    else if code == 1 { "warn" }
    else if code == 2 { "error" }
    // ...
}

// After: 查表 → CC=1
static LABELS: [&str; 3] = ["ok", "warn", "error"];
fn status_label(code: u8) -> &'static str {
    LABELS.get(code as usize).copied().unwrap_or("unknown")
}
```

### 补测试的优先级

不是所有未覆盖函数都同等重要。优先补：

1. **公共 API 函数**（`pub fn`，被多个调用方依赖）
2. **错误处理路径**（`Err(_)` 分支、`?` 传播后的恢复逻辑）
3. **边界条件**（空输入、极值、并发竞态）
4. 内部 helper（最后补，或通过上层测试间接覆盖）

## Baseline 回归检测

防止 PR 让已有函数变差：

```bash
# main 上保存 baseline
cargo crap --lcov lcov.info --workspace --format json --output baseline.json

# PR 上检测
cargo crap --lcov lcov.info --workspace --baseline baseline.json --fail-regression
```

v0.2.x 的 delta 模式能识别函数移动（同签名+同 body），报告为 "Moved" 而非 New+Removed。

## 持久化配置

项目根目录 `.cargo-crap.toml`（全部字段可选，CLI 覆盖文件）：

```toml
threshold = 30.0
fail-above = true
missing = "pessimistic"
exclude = ["tests/**", "benches/**", "**/tests/**"]
allow = ["src/generated/**"]
jobs = 4
```

| 场景 | 做法 |
|------|------|
| 收紧门禁 | `threshold = 15.0` |
| 存量代码暂不修 | baseline + `--fail-regression`（只拦新增回归） |
| 忽略生成/测试代码 | `exclude` / `allow` |

## 排错

| 现象 | 处理 |
|------|------|
| 全部 0% 覆盖 | LCOV 路径与源码不对齐；同一 shell 连续跑 llvm-cov + crap |
| 存量代码全部超标 | 用 baseline + `--fail-regression` 渐进治理，不强求一次性清零 |
| 工具未安装 | 告知用户执行：`cargo install cargo-crap cargo-llvm-cov cargo-nextest && rustup component add llvm-tools-preview` |

## 本地清零复验（workspace CRAP ≤ 30）

```bash
bash scripts/ci-crap.sh
# 可选 baseline：bash scripts/ci-crap.sh --format json --output crap-baseline.json
```

通过标准：`--fail-above` exit 0，且无 `exceed CRAP threshold` 行。

## Agent 执行清单

1. 生成 lcov（Windows 用 `NEXTEST_PROFILE=ci`，见上文）
2. 运行 `cargo crap --lcov lcov.info --workspace --min 30`
3. 对每个超标函数：先看 CRAP 中的 CC 列和 Coverage 列——判断是复杂度问题还是覆盖率问题。（判断逻辑和修复模式见上方「超标函数的修复方法」）
4. 修复后重新生成 lcov 并复验 CRAP ≤ threshold。
5. **不要**调高 threshold 或加 `--allow` 来绕过超标。
6. 本 skill 是可选本地质量分析——不替代 `post-change-checklist` 的 fmt/clippy/nextest 闸门。
