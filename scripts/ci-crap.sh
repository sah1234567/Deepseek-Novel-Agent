#!/usr/bin/env bash
# Regenerate lcov.info then run cargo-crap. Must run both in the same shell — stale lcov
# after refactors yields 0% coverage and false CRAP failures.
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cd "$ROOT"

# shellcheck source=ci-nextest-env.sh
source "$ROOT/scripts/ci-nextest-env.sh"

export NEXTEST_PROFILE="${NEXTEST_PROFILE:-ci}"
THRESHOLD="${CRAP_THRESHOLD:-30}"

echo "=== cargo llvm-cov nextest → lcov.info (NEXTEST_PROFILE=$NEXTEST_PROFILE) ==="
cargo llvm-cov nextest --workspace --all-features --lcov --output-path lcov.info

echo "=== cargo crap --fail-above --threshold $THRESHOLD ==="
cargo crap --lcov lcov.info --workspace --fail-above --threshold "$THRESHOLD" "$@"
