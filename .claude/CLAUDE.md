# DeepSeek Novel Agent

AI-assisted long-form novel writing desktop application (Rust + Tauri 2 + React 18 + TypeScript).

## Architecture

```
ui (React 18 / Vite 8 / TypeScript) → Tauri IPC → novel-server → novel-core
                                                              ├── novel-deepseek (LLM)
                                                              ├── novel-tools (tool dispatch)
                                                              ├── novel-knowledge (RAG/scaffold)
                                                              ├── novel-state (SQLite sessions)
                                                              ├── novel-compaction (context mgmt)
                                                              ├── novel-config (paths/settings)
                                                              ├── novel-skills (runtime skills)
                                                              ├── novel-memory (memory extraction)
                                                              └── novel-logging (tracing)
```

- **Crate DAG:** `novel-server → (novel-core | novel-deepseek | novel-tools | ...) → novel-config`. No reverse deps.
- **IPC:** `invoke` commands + `listen`/`emit` events via `src-tauri/` + `novel-server/src/tauri/`. No direct WebSocket from ui.
- **Data:** Each work in `works/{name}/` gets its own `state.db`; global API config in `.novel-agent/api_config.json`.

## Key Conventions

- **Tests:** `cargo nextest run` ONLY — never `cargo test`. CI profile: `ci` (30s timeout).
- **Node:** 24+ (`ui/.nvmrc`, `engine-strict=true`). Use pnpm 11.15.1 (not npm/yarn).
- **Rust toolchain:** stable, with `rustfmt` + `clippy` + `llvm-tools-preview` components.
- **CRAP threshold:** 20 (`.cargo-crap.toml`). Never raise in production code.
- **Clippy:** `-D warnings -D clippy::unwrap_used -D clippy::expect_used` in all production paths.
- **Windows:** Use Git Bash for `.sh` scripts; `.ps1` wrappers call Git Bash internally.
- **No WSL bash:** Git Bash only on Windows.

## CI Gates

| Path Changed | Run | Script |
|-------------|-----|--------|
| `crates/**`, `src-tauri/**`, `tests/integration/**` | Rust gate (fmt → clippy → nextest → tauri → **crap**) | `scripts/ci-rust-gate.sh` |
| `ui/**` | Frontend gate | `scripts/ci-frontend.sh` |
| `Cargo.toml`, `Cargo.lock` | Security audit | `scripts/ci-security-audit.sh` |
| IPC/Fork/engine | Smoke test | `scripts/smoke-ipc-fork.sh` |

## Post-Change Skills

| What changed | Use skill |
|-------------|-----------|
| Only `crates/` (Rust backend) | [`rust-post-change`](.claude/skills/rust-post-change/SKILL.md) |
| Only `ui/` (frontend) | [`ui-post-change`](.claude/skills/ui-post-change/SKILL.md) |
| Only `src-tauri/` (Tauri shell) | [`tauri-post-change`](.claude/skills/tauri-post-change/SKILL.md) |
| Cross-layer (IPC/DTO/events) | [`post-change-checklist`](.claude/skills/post-change-checklist/SKILL.md) |
| IPC/Fork/engine events | [`smoke-post-change`](.claude/skills/smoke-post-change/SKILL.md) |
| Refactoring/cleanup | [`refactor-cleanup`](.claude/skills/refactor-cleanup/SKILL.md) |
| CRAP/coverage | [`cargo-crap`](.claude/skills/cargo-crap/SKILL.md) |
| Design/architecture | [`engineering-principles`](.claude/skills/engineering-principles/SKILL.md) |

## DeepSeek API Notes

- Tool parameter names MUST be snake_case (DeepSeek official convention).
- Streaming `function.arguments` first chunk is `""` — `parse_tool_arguments("")` returns `Ok({})`. Must check object is non-empty before emitting.
- API Key resolution: env `DEEPSEEK_API_KEY` > `.novel-agent/api_config.json`.
