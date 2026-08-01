---
name: cargo-crap
description: >-
  用 cargo-crap 计算 Rust 函数的 CRAP（Change Risk Anti-Patterns）分数：圈复杂度 × 覆盖率。
  项目脚本分两步：`ci-lcov` 生成 lcov.info（慢，改代码后跑一次），`ci-crap` 仅跑 cargo crap（快，可反复）。
  覆盖 `.cargo-crap.toml`、过期 lcov 假阳性、Windows PowerShell 陷阱、baseline 回归、超标函数修复。
  用户提到 CRAP、cargo crap、lcov、ci-crap、ci-lcov 时使用。本 skill 是 CRAP 门禁步骤的权威参考。
---

# cargo-crap：CRAP 复杂度 × 覆盖率分析

## 快速开始

```bash
# 全流程（改代码后首次）
bash scripts/ci-lcov.sh && bash scripts/ci-crap.sh

# 仅重跑门禁（lcov.info 新鲜时）
bash scripts/ci-crap.sh
```

```powershell
# Windows PowerShell
.\scripts\ci-lcov.ps1; .\scripts\ci-crap.ps1
```

> **Windows：** 入口用 `.\scripts\ci-crap.ps1`（内部调 Git Bash）；交互式 Git Bash 中亦可直接 `bash scripts/ci-crap.sh`（与 CLAUDE.md「Windows 用 Git Bash 跑 .sh」口径一致）。

## 两步脚本

| 脚本 | 做什么 | 耗时 | 何时跑 |
|------|--------|------|--------|
| **`ci-lcov`** | `cargo llvm-cov nextest` → `lcov.info` | 慢（全量编译 + 测试） | 改代码/测试后、首次 clone、增删文件 |
| **`ci-crap`** | **仅** `cargo crap --fail-above`（读已有 `lcov.info`） | 快（秒级） | 同一代码反复调参/排查 |

`lcov.info` 在 `.gitignore`，是本机覆盖率快照，不进 git。

## 环境变量

| 变量 | 默认 | 作用 |
|------|------|------|
| `LCOV_PATH` | `lcov.info` | 两步共用 |
| `CRAP_THRESHOLD` | `20` | 仅 `ci-crap`（与 `.cargo-crap.toml` `threshold` 一致） |
| `NEXTEST_PROFILE` | `ci` | 仅 `ci-lcov` |

## 踩坑速查

### 坑 1：只跑裸 `cargo crap`，exit 0 不代表通过

| 命令 | exit | 行为 |
|------|------|------|
| `cargo crap --lcov lcov.info` | 常为 0 | 仅浏览全表；`✗ N exceed` 不会失败 |
| `cargo crap --lcov lcov.info --workspace --min 20` | 0 | 排查用 |
| `cargo crap --lcov lcov.info --workspace --fail-above --threshold 20` | 超标 → 1 | **门禁**（`ci-crap` 等价） |

**规则：** 门禁用 `ci-crap` 脚本或显式带 `--fail-above --workspace`。

### 坑 2：过期 `lcov.info` → 假阳性 CRAP 爆表

**信号（stderr 警告）：**
```
warning: N source files had no matching entry in the LCOV report
  crates\novel-knowledge\src\findings.rs
  crates\novel-tools\src\novel\chapter_lint.rs
```

**原因：** 新文件/移动文件在旧 lcov 中覆盖率为 0%，CC 15–20 → CRAP 200–300+。

| 必须重跑 `ci-lcov` | 只需跑 `ci-crap` |
|---------------------|-------------------|
| 改了 `crates/` 源码或测试 | 同代码反复调参 |
| 增删/移动文件 | `lcov.info` 匹配当前 commit |
| 首次 clone 或删了 `lcov.info` | |
| 出现 **no matching entry** 警告 | |
| 单函数 0% 覆盖 + CRAP>100 | |

**快速自检：** `git diff` 触达 `crates/` 且未跑过 `ci-lcov` → 先 lcov 再 crap。

### 坑 3：Windows `ci-lcov.ps1` 误报 / OOM 崩溃

**stderr 误报：** `cargo llvm-cov` 的 `info: cargo-llvm-cov currently setting cfg(coverage)` 可能被 PowerShell `Stop` 策略截断。脚本已用 `Continue` 包裹。若仍失败：

```powershell
$env:NEXTEST_PROFILE = "ci"
cargo llvm-cov nextest --workspace --all-features --lcov --output-path lcov.info
```

**OOM 崩溃（Windows 特有）：** instrument-coverage 编译大 crate（如 novel-core 测试目标）在高并行下 rustc 内存耗尽崩溃——症状：`handle_alloc_error` / exit `0xc0000409`，随后 `error[E0786]: found invalid metadata files` 或 `crate X required to be available in rlib format, but was not found`（崩溃残留）。缓解（脚本已内置）：

- `CARGO_BUILD_JOBS=2`（降并行）+
- `CARGO_INCREMENTAL=0`（关增量，避免 rmeta-only 残留）
- `--exclude novel-agent`（src-tauri 的 tauri-build 在 instrument-coverage 下不稳定，且已被 `.cargo-crap.toml` 排除）
- `--features tauri`（novel-server 的 event_payload/dto 模块是 `#[cfg(feature="tauri")]`，不开则无覆盖数据 → CRAP 假阳性）

清 `target/llvm-cov-target` 后重跑。成功标志：`Finished report saved to lcov.info`。

### 坑 4：`.sh` 脚本报 `pipefail: invalid option`

CRLF 行尾导致。`ci-lcov.sh` / `ci-crap.sh` 已内置 `if (set -o pipefail)` 防护（不会报错）；若其他 `.sh` 脚本复现，修复：`git checkout -- scripts/<脚本名>.sh`。

### 坑 5：工具未安装

```bash
cargo install cargo-crap cargo-llvm-cov cargo-nextest
rustup component add llvm-tools-preview
```

## 超标函数修复指南

看报告 **CRAP / CC / Coverage** 三列对症：

| 信号 | 修复方向 | 示例 |
|------|---------|------|
| CRAP 高 + **Coverage 0%** + 刚加文件 | **先 `ci-lcov`**；仍超标再补测 | `permission.rs` 新文件伪报 306 |
| CRAP 高 + CC 低 | 补测试覆盖 | 加 `#[cfg(test)]` 单测 |
| CRAP 中 + CC 高（≳14） | **拆函数**降 CC | `evaluate_tool_permissions` → 拆出 `evaluate_plan_mode` + `evaluate_standard_mode` |
| CRAP 高 + CC 也高 | 先拆再补 | 长函数 → 提取 helper + 单测 |
| 长 loop 多路 early return | 提取判定函数 | `run_inner_turn_loop` → 提取 `inner_turn_loop_exit_reason` |

**策略：** 公共 API、错误路径、边界条件优先补测。纯逻辑提取为无 IO 的 `fn` 便于单测。

**禁止：** 调高 `threshold`、`--allow` 绕过、或把生产文件塞进 `exclude`。

## Baseline 回归（存量项目推荐）

```bash
# 建立基线
cargo crap --lcov lcov.info --workspace --format json --output baseline.json

# 后续门禁：只拦截 CRAP 上升
cargo crap --lcov lcov.info --workspace --baseline baseline.json --fail-regression
```

v0.2.x 能识别函数移动（报告 Moved，非 New+Removed）。适合存量代码难以一次性清零的场景。

## `.cargo-crap.toml`（项目已配置，以磁盘文件为准）

```toml
threshold = 20
missing = "pessimistic"   # 0% 覆盖 → CRAP 极敏感
exclude = [
  "**/src-tauri/**",           # Tauri 壳（IPC 胶水，业务在 session_api/event_payload/dto）
  "**/tauri/commands/**",
  "**/tauri/engine_loop.rs",
  "**/tauri/events.rs",
  "**/tauri/graph_emit.rs",
  "**/tauri/mod.rs",
  "**/build.rs",
  "**/tests/**",               # 测试目录一律排除（须带 **/ 前缀）
  "**/tests/integration/**",
  "**/engine/tests.rs",        # src/ 下内联 #[cfg(test)] 模块
  "**/turn/loop/tests.rs",
  "**/db/tests.rs",
]
```

| 场景 | 做法 |
|------|------|
| 存量难清零 | baseline + `--fail-regression` |
| 收紧门禁 | 降 `threshold` 或改 `CRAP_THRESHOLD` 环境变量 |
| 新增排除项 | 追加到 `.cargo-crap.toml` 的 `exclude`（**须带** `**/` 前缀），禁止把生产文件塞入 |

## 手动命令

```bash
# 1. 覆盖率快照
export NEXTEST_PROFILE=ci
cargo llvm-cov nextest --workspace --all-features --lcov --output-path lcov.info

# 2. 排查超标（查看哪些函数超 threshold）
cargo crap --lcov lcov.info --workspace --min 20

# 3. 门禁
cargo crap --lcov lcov.info --workspace --fail-above --threshold 20
```

单 crate：`cargo llvm-cov nextest -p <crate> --lcov --output-path lcov.info` + `cargo crap --lcov lcov.info --path crates/<name>`。

## CRAP 公式

```
CRAP(m) = comp(m)² × (1 − cov(m)/100)³ + comp(m)
```

- CC=1 且 100% 覆盖 → CRAP=1.0
- 100% 覆盖时 CRAP=CC（正常）
- CC≳20 → 极难低于默认阈值 20
- 0% 覆盖 + CC=17 → CRAP≈306

## Agent 执行清单

1. **判新鲜度：** `git diff` 触达 `crates/` → 先 `ci-lcov`；仅调参 → 直跑 `ci-crap`
2. **选平台：** Windows → `.\scripts\ci-*.ps1`；Unix → `bash scripts/ci-*.sh`
3. **门禁：** `ci-crap`；**排查：** `cargo crap --workspace --min 20`，注意 stderr LCOV 警告
4. **修复：** 按 CC/Coverage 表对症；修后必须 `ci-lcov` → `ci-crap`
5. **不绕过：** 不调高 threshold、不加 `--allow`、不把生产文件加 exclude

**通过标准：** `ci-crap` exit 0，且输出**无** `✗ N function(s) exceed CRAP threshold` 行（有该行即超标，exit 非 0）。
