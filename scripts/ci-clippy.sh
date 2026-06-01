#!/usr/bin/env bash
# Clippy gate: full workspace (lib + tests + bins); deny all warnings.
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cd "$ROOT"

echo "=== cargo clippy --workspace --all-targets -- -D warnings -D clippy::unwrap_used ==="
cargo clippy --workspace --all-targets -- -D warnings -D clippy::unwrap_used
