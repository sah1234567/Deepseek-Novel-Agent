#!/usr/bin/env bash
# Decide whether ci-lcov must regenerate lcov.info.
# Exit 0 → need regenerate; exit 1 → reuse existing report.
#
# Env:
#   LCOV_PATH       default lcov.info
#   CI_SKIP_LCOV=1  always reuse (fail later in ci-crap if missing)
#   CI_FORCE_LCOV=1 always regenerate
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cd "$ROOT"

LCOV_PATH="${LCOV_PATH:-lcov.info}"

if [[ "${CI_FORCE_LCOV:-}" == "1" ]]; then
  echo "=== ci-lcov-needed: CI_FORCE_LCOV=1 → regenerate ==="
  exit 0
fi

if [[ "${CI_SKIP_LCOV:-}" == "1" ]]; then
  echo "=== ci-lcov-needed: CI_SKIP_LCOV=1 → reuse ${LCOV_PATH} ==="
  exit 1
fi

if [[ ! -f "$LCOV_PATH" ]]; then
  echo "=== ci-lcov-needed: missing ${LCOV_PATH} → regenerate ==="
  exit 0
fi

# Any Rust/Cargo source newer than the report → stale.
# Git Bash find supports -newer; keep roots aligned with CRAP / nextest coverage inputs.
newer="$(
  find crates src-tauri tests/integration \
    \( -name '*.rs' -o -name 'Cargo.toml' -o -name 'Cargo.lock' \) \
    -newer "$LCOV_PATH" 2>/dev/null | head -n 1 || true
)"
if [[ -n "$newer" ]]; then
  echo "=== ci-lcov-needed: stale vs ${newer} → regenerate ==="
  exit 0
fi

# Config that changes which functions are scored / threshold.
if [[ -f .cargo-crap.toml ]] && [[ .cargo-crap.toml -nt "$LCOV_PATH" ]]; then
  echo "=== ci-lcov-needed: .cargo-crap.toml newer → regenerate ==="
  exit 0
fi

echo "=== ci-lcov-needed: ${LCOV_PATH} fresh → reuse ==="
exit 1
