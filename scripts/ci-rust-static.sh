#!/usr/bin/env bash
# Rust static gate: rustfmt only (cargo check is covered by clippy in ci-clippy.sh).
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cd "$ROOT"

echo "=== rustfmt ==="
cargo fmt --all -- --check

# Ensure ui/dist + icons exist so Tauri crate can be checked/clippy'd/tested.
bash "$ROOT/scripts/ci-ui-dist.sh"
bash "$ROOT/scripts/ci-tauri-icons.sh"
