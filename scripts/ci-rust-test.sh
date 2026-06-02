#!/usr/bin/env bash
# Rust CI test gate: nextest workspace with stricter CI slow-timeout profile.
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cd "$ROOT"

bash "$ROOT/scripts/ci-ui-dist.sh"
bash "$ROOT/scripts/ci-tauri-icons.sh"

# shellcheck source=ci-nextest-env.sh
source "$ROOT/scripts/ci-nextest-env.sh"

# Stress DB concurrency on every OS (Linux GHA used to be the only place this flaked).
echo "=== novel-state: concurrent_writes (explicit, --test-threads=10) ==="
cargo nextest run -p novel-state -E 'test(concurrent_writes)' --profile ci --test-threads=10

echo "=== cargo nextest run --workspace --profile ci ==="
cargo nextest run --workspace --profile ci
