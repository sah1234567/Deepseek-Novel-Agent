# CI / CD 脚本

本地与 [`.github/workflows/ci.yml`](../.github/workflows/ci.yml) 共用同一套 bash 脚本。**Node.js 固定 24**（`ui/.nvmrc` + `ci-check-node.sh`）。

**GHA PR 覆盖：** Ubuntu `frontend` + 三平台 `ci-rust-gate.sh` + Ubuntu `security-audit`。macOS runner 不重复 Vitest（非漏测）；`cargo audit` 不在 Win/mac job 重复（同 `Cargo.lock`）。

## 本地入口（PowerShell）

| 命令 | 等同 GitHub |
|------|-------------|
| `.\scripts\ci-windows.ps1` | GHA `rust-windows`（frontend + Rust）+ 本地另含 `ci-security-audit`（GHA audit 仅 Ubuntu `security-audit` job） |
| `.\scripts\ci-local.ps1` | Windows → 上表；Linux/macOS → `ci-pr-gate` |

需 **Git for Windows** 的 `bash.exe`（勿用 WSL，否则找不到 Windows 版 `cargo`）。

## 本地入口（Bash）

```bash
bash scripts/ci-pr-gate.sh      # 全量（≈ Ubuntu 三 job + Win 前端复测；见 ci.yml 顶注释）
bash scripts/ci-rust-gate.sh    # Rust + Tauri（CI 用；日常改单域不必全跑）
bash scripts/ci-frontend.sh     # 仅 Vitest + build
bash scripts/ci-tauri.sh        # 仅 Tauri 壳 check + build
bash scripts/ci-security-audit.sh
```

**按改动范围分域验证**（与 [post-change-checklist](../.claude/skills/post-change-checklist/SKILL.md) 步骤 0 一致）：

| 只改了… | 跑 |
|--------|-----|
| `crates/`、`tests/integration/` | `ci-rust-static.sh` → `ci-clippy.sh` → `ci-rust-test.sh` |
| `src-tauri/`（或 Tauri IPC 需编壳） | `ci-tauri.sh` |
| `ui/` | `ci-frontend.sh` |
| `Cargo.lock` / 发 PR | 上表相关项 + 可选 `ci-security-audit.sh` |

`ci-security-audit.sh`：若已有 `~/.cargo/advisory-db` 则默认 **不** 连 GitHub（避免大陆网络下 `cargo audit` 打印 `error:` 后仍用缓存通过）。需刷新库：`AUDIT_FORCE_FETCH=1 bash scripts/ci-security-audit.sh`。


## 脚本一览

| 脚本 | 用途 |
|------|------|
| `ci-check-node.sh` | 强制 Node ≥24（npm 步骤前调用） |
| `ci-frontend.sh` | `npm ci` + `npm audit --audit-level=critical` + test + build |
| `ci-ui-dist.sh` | 构建 `ui/dist`（Tauri 编译前） |
| `ci-tauri-icons.sh` | 占位 `icon.png` / `icon.ico` |
| `ci-rust-static.sh` | rustfmt + `cargo check --workspace` |
| `ci-clippy.sh` | clippy（deny warnings + unwrap_used） |
| `ci-nextest-env.sh` | 统一 `NEXTEST_TEST_THREADS` |
| `ci-rust-test.sh` | nextest（含 `concurrent_writes` 压测） |
| `ci-crap.sh` / `ci-crap.ps1` | **仅** `cargo crap --fail-above`（读已有 `lcov.info`） |
| `ci-lcov.sh` / `ci-lcov.ps1` | 生成 `lcov.info`（`llvm-cov nextest`；重构后须先跑再 `ci-crap`） |
| （本地 CRAP 全流程） | `bash scripts/ci-lcov.sh && bash scripts/ci-crap.sh`；Windows：`.\scripts\ci-lcov.ps1; .\scripts\ci-crap.ps1` |
| `ci-tauri-check.sh` | `cargo check` novel-agent |
| `ci-tauri.sh` | check + `cargo build` novel-agent |
| `ci-linux-tauri-deps.sh` | GHA Ubuntu：WebKit/GTK apt |
| `ci-rust-gate.sh` | 上述 Rust/Tauri 子步骤组合 |
| `ci-security-audit.sh` | `cargo audit --deny warnings`（传递依赖见 `.cargo/audit.toml`） |
| `ci-gate-core.sh` | frontend + rust-gate + audit |
| `ci-pr-gate.sh` | → `ci-gate-core` |
| `ci-windows-gate.sh` | → `ci-gate-core` |
| `ci-require-toolchain.sh` | Git Bash 下校验 `cargo` 在 PATH |

## 维护脚本（非 CI）

| 脚本 | 用途 |
|------|------|
| `reset-work-databases.ps1` / `.sh` | 删除 `works/*/.novel-agent/state.db` 后由应用重建。**升级后若移除旧 transcript/tool 显示兼容层，或 metadata 报 legacy format、无法 resume，须先跑此脚本再开新会话**（与 `novel-state` metadata 策略一致） |

## Node 24

- CI：`setup-node` + `node-version-file: ui/.nvmrc`（值为 `24`）
- 本地：User PATH 中 Node 24 优先于 Cursor 自带 Node 22；`npm` 步骤由 `ci-check-node.sh` 校验
