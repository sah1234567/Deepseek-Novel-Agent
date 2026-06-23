---
name: cargo-crap
description: >-
  用 cargo-crap 计算 Rust 函数的 CRAP（Change Risk Anti-Patterns）分数：圈复杂度 × 覆盖率。
  项目脚本分两步：`ci-lcov` 生成 lcov.info（慢，改代码后跑一次），`ci-crap` 仅跑 cargo crap（快，可反复）。
  覆盖 `.cargo-crap.toml`、过期 lcov 假阳性、Windows PowerShell 陷阱、baseline 回归、超标函数修复。
  用户提到 CRAP、cargo crap、lcov、ci-crap、ci-lcov 时使用。
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

> **Windows 勿用** `./scripts/ci-crap.sh`：`.sh` 关联 `git-bash.exe` 会弹新窗，当前终端无输出。用 `.\scripts\ci-crap.ps1`。

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
  crates\novel-tools\src\permission.rs
  crates\novel-knowledge\src\text_util.rs
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

### 坑 3：Windows `ci-lcov.ps1` 误报

`cargo llvm-cov` stderr 的 `info: cargo-llvm-cov currently setting cfg(coverage)` 可能被 PowerShell `Stop` 策略截断。脚本已用 `Continue` 包裹。若仍失败：

```powershell
$env:NEXTEST_PROFILE = "ci"
cargo llvm-cov nextest --workspace --all-features --lcov --output-path lcov.info
```

成功标志：`Finished report saved to lcov.info`。

### 坑 4：`.sh` 脚本报 `pipefail: invalid option`

CRLF 行尾导致。修复：`git checkout -- scripts/ci-*.sh`

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

## `.cargo-crap.toml`（项目已配置）

```toml
threshold = 20
missing = "pessimistic"   # 0% 覆盖 → CRAP 极敏感
exclude = ["src-tauri", "**/tests/integration/**", ...]
```

| 场景 | 做法 |
|------|------|
| 存量难清零 | baseline + `--fail-regression` |
| 收紧门禁 | 降 `threshold` 或改 `CRAP_THRESHOLD` 环境变量 |
| integration 测试 | `**/tests/integration/**` 排除（**须带** `**/` 前缀） |

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

**通过标准：** `ci-crap` exit 0，输出含 `none exceed CRAP threshold`。
