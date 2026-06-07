#!/usr/bin/env bash
# CRAP gate only: cargo crap --fail-above (reads existing lcov.info).
#
# Generate lcov first: bash scripts/ci-lcov.sh
# Usage: bash scripts/ci-crap.sh [--summary | other cargo-crap flags...]
#
# Windows PowerShell: .\scripts\ci-crap.ps1

set -e
set -u
if (set -o pipefail) 2>/dev/null; then
  set -o pipefail
fi

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cd "$ROOT"

THRESHOLD="${CRAP_THRESHOLD:-30}"
LCOV_PATH="${LCOV_PATH:-lcov.info}"

exec cargo crap --lcov "$LCOV_PATH" --workspace --fail-above --threshold "$THRESHOLD" "$@"
