#!/usr/bin/env bash
# Generate lcov.info for ci-crap (not part of the CRAP gate itself).
#
# Usage: bash scripts/ci-lcov.sh

set -e
set -u
if (set -o pipefail) 2>/dev/null; then
  set -o pipefail
fi

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cd "$ROOT"

export NEXTEST_PROFILE="${NEXTEST_PROFILE:-ci}"
LCOV_PATH="${LCOV_PATH:-lcov.info}"

exec cargo llvm-cov nextest --workspace --all-features --lcov --output-path "$LCOV_PATH"
