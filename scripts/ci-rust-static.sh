#!/usr/bin/env bash
# Rust static gate: rustfmt + cargo check (matches ci.yml rust job).
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cd "$ROOT"

echo "=== rustfmt ==="
cargo fmt --all -- --check

bash "$ROOT/scripts/ci-ui-dist.sh"
bash "$ROOT/scripts/ci-tauri-icons.sh"

echo "=== cargo check --workspace ==="
cargo check --workspace
