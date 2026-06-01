#!/usr/bin/env bash
# Rust CI test gate: nextest workspace with stricter CI slow-timeout profile.
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cd "$ROOT"

bash "$ROOT/scripts/ci-ui-dist.sh"
bash "$ROOT/scripts/ci-tauri-icons.sh"

echo "=== cargo nextest run --workspace --profile ci ==="
cargo nextest run --workspace --profile ci
