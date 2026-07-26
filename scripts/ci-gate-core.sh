#!/usr/bin/env bash
# Shared PR-level CI steps (frontend + Rust static/clippy/test + Tauri smoke + audit).
# Invoked by ci-pr-gate.sh (Linux/Ubuntu) and ci-windows-gate.sh (Windows).
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cd "$ROOT"

# Do not default CARGO_BUILD_JOBS=1 — GHA sets it; local uses cargo's default parallelism.
export CARGO_TERM_COLOR="${CARGO_TERM_COLOR:-always}"
export RUST_BACKTRACE="${RUST_BACKTRACE:-1}"
if [[ -n "${CARGO_BUILD_JOBS:-}" ]]; then
  echo "=== CARGO_BUILD_JOBS=${CARGO_BUILD_JOBS} ==="
fi

bash "$ROOT/scripts/ci-check-node.sh"

job_banner() {
  echo ""
  echo "========================================"
  echo " JOB: $1"
  echo "========================================"
}

job_banner "frontend (Vitest + build)"
bash "$ROOT/scripts/ci-frontend.sh"

job_banner "rust (fmt + check + clippy + nextest + tauri)"
bash "$ROOT/scripts/ci-rust-gate.sh"

job_banner "security-audit"
bash "$ROOT/scripts/ci-security-audit.sh"
