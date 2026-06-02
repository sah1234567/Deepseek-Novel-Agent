#!/usr/bin/env bash
# Tauri shell smoke: check + cargo build novel-agent (all platforms; Linux CI installs WebKit deps).
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"

bash "$ROOT/scripts/ci-tauri-check.sh"

bash "$ROOT/scripts/ci-linux-tauri-deps.sh"

echo "=== cargo build novel-agent (src-tauri) ==="
cd "$ROOT"
cargo build -p novel-agent --manifest-path src-tauri/Cargo.toml
