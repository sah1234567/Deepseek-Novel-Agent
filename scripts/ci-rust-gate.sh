#!/usr/bin/env bash
# Rust gate shared by Ubuntu / Windows / macOS CI (and local platform checks).
# Same steps everywhere: rustfmt + check → clippy → nextest → Tauri shell build.
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cd "$ROOT"

export CARGO_BUILD_JOBS="${CARGO_BUILD_JOBS:-1}"
export CARGO_TERM_COLOR="${CARGO_TERM_COLOR:-always}"
export RUST_BACKTRACE="${RUST_BACKTRACE:-1}"

bash "$ROOT/scripts/ci-linux-tauri-deps.sh"
bash "$ROOT/scripts/ci-rust-static.sh"
bash "$ROOT/scripts/ci-clippy.sh"
bash "$ROOT/scripts/ci-rust-test.sh"
bash "$ROOT/scripts/ci-tauri.sh"

echo "=== ci-rust-gate passed ==="
