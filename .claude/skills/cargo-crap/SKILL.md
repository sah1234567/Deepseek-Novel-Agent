---
name: cargo-crap
description: >-
  用 cargo-crap 计算 Rust 函数的 CRAP（Change Risk Anti-Patterns）分数：圈复杂度 × 覆盖率。
  项目脚本分两步：`ci-lcov` 生成 lcov.info（慢，改代码后跑一次），`ci-crap` 仅跑 cargo crap（快，可反复）。
  覆盖 `.cargo-crap.toml`、baseline 回归、超标函数修复。用户提到 CRAP、cargo crap、lcov、ci-crap、ci-lcov 时使用。
---

# cargo-crap：CRAP 复杂度 × 覆盖率分析

## 是什么

`cargo crap`（v0.2.2）合成**圈复杂度（CC）**与**行覆盖率**为 CRAP 分数：

```text
CRAP(m) = comp(m)² × (1 − cov(m)/100)³ + comp(m)
```

- CC=1 且 100% 覆盖 → CRAP = 1.0
- 100% 覆盖时 CRAP = CC（正常现象）
- CC ≳ 30 → 很难低于默认阈值 30

源码：[minikin/cargo-crap](https://github.com/minikin/cargo-crap) · [docs.rs](https://docs.rs/cargo-crap/latest/cargo_crap/)

## 项目脚本（两步分离）

| 脚本 | 做什么 | 耗时 | 何时跑 |
|------|--------|------|--------|
| **`ci-lcov`**（`.sh` / `.ps1`） | `cargo llvm-cov nextest` → 覆盖写入 `lcov.info` | 慢（全 workspace 编译 + nextest） | **改 Rust 源码/测试后**，或尚无 `lcov.info` |
| **`ci-crap`**（`.sh` / `.ps1`） | **仅** `cargo crap --fail-above`（读已有 `lcov.info`） | 快（秒级） | 同一版代码下可**反复**跑 |

`lcov.info` 在 `.gitignore`，不进 git，是本机覆盖率快照。

### 命令

```bash
# Linux / macOS / Git Bash
bash scripts/ci-lcov.sh          # 拍快照（每个代码版本一次）
bash scripts/ci-crap.sh          # CRAP 门禁（可多次）
bash scripts/ci-crap.sh --summary
bash scripts/ci-lcov.sh && bash scripts/ci-crap.sh   # 全流程
```

```powershell
# Windows PowerShell（当前终端输出，不经 bash）
.\scripts\ci-lcov.ps1
.\scripts\ci-crap.ps1
.\scripts\ci-lcov.ps1; .\scripts\ci-crap.ps1
```

> **Windows 勿用** `./scripts/ci-crap.sh`：`.sh` 关联 `git-bash.exe` 会弹新窗，当前终端无输出。用 `.\scripts\ci-crap.ps1`。

### 环境变量

| 变量 | 默认 | 作用 |
|------|------|------|
| `NEXTEST_PROFILE` | `ci` | 仅 **ci-lcov** 使用 |
| `LCOV_PATH` | `lcov.info` | 两个脚本共用路径 |
| `CRAP_THRESHOLD` | `30` | 仅 **ci-crap** 使用 |

### lcov 何时必须重生

**不是一辈子只跑一次**，而是**每个代码版本对应一次 ci-lcov**：

| 需要重跑 `ci-lcov` | 不必重跑，只跑 `ci-crap` |
|--------------------|---------------------------|
| 改了 `crates/` 源码、增删/移动文件 | 同一段代码，反复查 CRAP / 调参 |
| 改了测试 | 已有新鲜 `lcov.info` |
| 删了 `lcov.info` 或 clone 后首次 | |
| CRAP 大量 0% 覆盖（路径对不上旧 lcov） | |

重构后旧 `lcov.info` 路径错位会导致覆盖率按 0% 计、CRAP 虚高（本项目曾出现 68 个假阳性，重生 lcov 后归零）。

## 手动命令（不用脚本时）

```bash
# 1. 覆盖率（禁止 cargo test，只用 nextest）
export NEXTEST_PROFILE=ci   # Windows PowerShell: $env:NEXTEST_PROFILE = "ci"
cargo llvm-cov nextest --workspace --all-features --lcov --output-path lcov.info

# 2. 分析
cargo crap --lcov lcov.info --workspace --min 30              # 浏览
cargo crap --lcov lcov.info --workspace --fail-above --threshold 30  # 门禁
```

单 crate：`cargo llvm-cov --lcov -o lcov.info` + `cargo crap --lcov lcov.info --path crates/<name>`。

## CLI 标志（常用）

| 标志 | 作用 |
|------|------|
| `--threshold <N>` | 超标线，默认 30 |
| `--min <N>` | 只显示 CRAP ≥ N |
| `--fail-above` | 超标 exit 1（ci-crap 脚本已带） |
| `--baseline <json>` | 与上次 JSON 报告对比 |
| `--fail-regression` | 分数上升则 exit 1 |
| `--format <fmt>` | `human` / `json` / `github` / `markdown` / `pr-comment` / `sarif` |

其余见 `cargo crap --help`。

## 超标函数怎么修

| 信号 | 做法 |
|------|------|
| CRAP 高、CC 低 | 补测试 |
| CRAP 中、CC 高 | 拆函数降复杂度 |
| CRAP 高、CC 也高 | 先拆再补 |

优先补：公共 API、错误路径、边界条件；内部 helper 可靠上层间接覆盖。

常见手法：提取 match 分支为独立函数、拆 parse/validate/execute、长 if-else 改查表（详见历史版本或 cargo-crap 文档）。

## Baseline 回归

```bash
cargo crap --lcov lcov.info --workspace --format json --output baseline.json
cargo crap --lcov lcov.info --workspace --baseline baseline.json --fail-regression
```

v0.2.x 能识别函数移动（报告 Moved，非 New+Removed）。

## `.cargo-crap.toml`

项目已配置 `threshold = 30`、`missing = "pessimistic"` 及 `exclude`（`src-tauri`、测试模块等）。CLI 可覆盖文件。

| 场景 | 做法 |
|------|------|
| 存量难一次性清零 | baseline + `--fail-regression` |
| 收紧门禁 | 降 `threshold` 或改脚本 `CRAP_THRESHOLD` |
| **禁止** | 为过关随意调高 threshold 或 `--allow` 绕过 |

## 排错

| 现象 | 处理 |
|------|------|
| 全部 0% 覆盖 | 重跑 `ci-lcov`；勿在路径错位时用旧 lcov |
| `ci-crap` 报找不到 lcov | 先 `ci-lcov` |
| Windows 弹新窗无输出 | 用 `.\scripts\ci-crap.ps1`，勿 `./scripts/ci-crap.sh` |
| `.sh` 报 `pipefail: invalid option` | CRLF 行尾；`git checkout -- scripts/ci-*.sh` |
| 工具未安装 | `cargo install cargo-crap cargo-llvm-cov cargo-nextest`；`rustup component add llvm-tools-preview` |

## Agent 执行清单

1. **判断是否已有与当前 diff 匹配的 `lcov.info`** — 仅改 crap 参数/浏览报告 → 只跑 `ci-crap`；改了 `crates/` 或测试 → 先 `ci-lcov` 再 `ci-crap`。
2. Windows 一律 `.\scripts\ci-lcov.ps1` / `.\scripts\ci-crap.ps1`；Unix 用 `bash scripts/ci-*.sh`。
3. 门禁：`ci-crap`（等价 `--fail-above --threshold 30`）；排查：`ci-crap.sh --min 30` 或 `--summary`。
4. 超标项看 CC 与 Coverage 列，按上表决定补测或重构；修完后 **必须** 重跑 `ci-lcov` 再 `ci-crap`。
5. **不要**调高 threshold / 乱加 `--allow` 绕过。
6. 本 skill 为**可选**本地质量分析，**不替代** `post-change-checklist` 的 fmt / clippy / nextest。

**通过标准：** `ci-crap` exit 0，无 `exceed CRAP threshold` 行。
