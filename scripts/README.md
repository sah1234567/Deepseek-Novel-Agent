# CI / CD 脚本

本地与 [`.github/workflows/ci.yml`](../.github/workflows/ci.yml) 共用同一套 bash 脚本。**Node.js 固定 24**（`ui/.nvmrc` + `ci-check-node.sh`）。

## 本地入口（PowerShell）

| 命令 | 等同 GitHub |
|------|-------------|
| `.\scripts\ci-windows.ps1` | `rust-windows`（frontend + Rust gate + audit） |
| `.\scripts\ci-local.ps1` | Windows → 上表；Linux/macOS → `ci-pr-gate` |

需 **Git for Windows** 的 `bash.exe`（勿用 WSL，否则找不到 Windows 版 `cargo`）。

## 本地入口（Bash）

```bash
bash scripts/ci-pr-gate.sh      # 全量（≈ Ubuntu PR jobs 合并）
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


## 脚本一览

| 脚本 | 用途 |
|------|------|
| `ci-check-node.sh` | 强制 Node ≥24（npm 步骤前调用） |
| `ci-frontend.sh` | `npm ci` + test + build |
| `ci-ui-dist.sh` | 构建 `ui/dist`（Tauri 编译前） |
| `ci-tauri-icons.sh` | 占位 `icon.png` / `icon.ico` |
| `ci-rust-static.sh` | rustfmt + `cargo check --workspace` |
| `ci-clippy.sh` | clippy（deny warnings + unwrap_used） |
| `ci-nextest-env.sh` | 统一 `NEXTEST_TEST_THREADS` |
| `ci-rust-test.sh` | nextest（含 `concurrent_writes` 压测） |
| `ci-tauri-check.sh` | `cargo check` novel-agent |
| `ci-tauri.sh` | check + `cargo build` novel-agent |
| `ci-linux-tauri-deps.sh` | GHA Ubuntu：WebKit/GTK apt |
| `ci-rust-gate.sh` | 上述 Rust/Tauri 子步骤组合 |
| `ci-security-audit.sh` | `cargo audit` |
| `ci-gate-core.sh` | frontend + rust-gate + audit |
| `ci-pr-gate.sh` | → `ci-gate-core` |
| `ci-windows-gate.sh` | → `ci-gate-core` |
| `ci-require-toolchain.sh` | Git Bash 下校验 `cargo` 在 PATH |

## 维护脚本（非 CI）

| 脚本 | 用途 |
|------|------|
| `reset-work-databases.ps1` / `.sh` | 清理作品 `state.db` |

## Node 24

- CI：`setup-node` + `node-version-file: ui/.nvmrc`（值为 `24`）
- 本地：User PATH 中 Node 24 优先于 Cursor 自带 Node 22；`npm` 步骤由 `ci-check-node.sh` 校验
