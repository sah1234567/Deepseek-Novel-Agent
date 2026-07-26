#!/usr/bin/env bash
# Rust gate shared by Ubuntu / Windows / macOS CI (and local platform checks).
# Same steps everywhere: rustfmt + check → clippy → nextest → Tauri shell build.
#
# Parallelism: GHA sets CARGO_BUILD_JOBS=1 (runner memory). Local leaves it unset
# so cargo uses all cores — do not default to 1 here.
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cd "$ROOT"

export CARGO_TERM_COLOR="${CARGO_TERM_COLOR:-always}"
export RUST_BACKTRACE="${RUST_BACKTRACE:-1}"
if [[ -n "${CARGO_BUILD_JOBS:-}" ]]; then
  echo "=== CARGO_BUILD_JOBS=${CARGO_BUILD_JOBS} ==="
fi

bash "$ROOT/scripts/ci-linux-tauri-deps.sh"
bash "$ROOT/scripts/ci-rust-static.sh"
bash "$ROOT/scripts/ci-clippy.sh"
bash "$ROOT/scripts/ci-rust-test.sh"
bash "$ROOT/scripts/ci-tauri.sh"
# CRAP gate: coverage → complexity×coverage fail-above threshold 20
# Skip llvm-cov when lcov.info is still fresh (local); GHA clean checkout always regenerates.
# Override: CI_FORCE_LCOV=1 / CI_SKIP_LCOV=1 (see ci-lcov-needed.sh).
if bash "$ROOT/scripts/ci-lcov-needed.sh"; then
  bash "$ROOT/scripts/ci-lcov.sh"
else
  echo "=== ci-lcov skipped (reuse existing lcov.info) ==="
fi
bash "$ROOT/scripts/ci-crap.sh"

echo "=== ci-rust-gate passed ==="
