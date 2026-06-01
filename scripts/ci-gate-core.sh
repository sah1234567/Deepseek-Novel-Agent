#!/usr/bin/env bash
# Shared PR-level CI steps (frontend + Rust static/clippy/test + Tauri smoke + audit).
# Invoked by ci-pr-gate.sh (Linux/Ubuntu) and ci-windows-gate.sh (Windows).
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cd "$ROOT"

export CARGO_BUILD_JOBS="${CARGO_BUILD_JOBS:-1}"
export CARGO_TERM_COLOR="${CARGO_TERM_COLOR:-always}"
export RUST_BACKTRACE="${RUST_BACKTRACE:-1}"

job_banner() {
  echo ""
  echo "========================================"
  echo " JOB: $1"
  echo "========================================"
}

job_banner "frontend (Vitest + build)"
bash "$ROOT/scripts/ci-frontend.sh"

job_banner "rust — static (rustfmt + check)"
bash "$ROOT/scripts/ci-rust-static.sh"

job_banner "rust — clippy"
bash "$ROOT/scripts/ci-clippy.sh"

job_banner "rust — nextest (CI profile)"
bash "$ROOT/scripts/ci-rust-test.sh"

job_banner "tauri-compile"
bash "$ROOT/scripts/ci-tauri.sh"

job_banner "security-audit"
bash "$ROOT/scripts/ci-security-audit.sh"
