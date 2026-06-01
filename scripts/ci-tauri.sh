#!/usr/bin/env bash
# Tauri compile smoke: frontend build + cargo build novel-agent (matches ci.yml tauri-compile job).
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"

bash "$ROOT/scripts/ci-ui-dist.sh"

# GitHub ubuntu-latest only; skip on local Windows/macOS and non-CI Linux.
if [ "${GITHUB_ACTIONS:-}" = "true" ] && [ "$(uname -s)" = "Linux" ]; then
  echo "=== Install Linux Tauri deps ==="
  sudo apt-get update
  sudo apt-get install -y \
    libwebkit2gtk-4.1-dev libgtk-3-dev libayatana-appindicator3-dev librsvg2-dev \
    libssl-dev libsoup-3.0-dev libjavascriptcoregtk-4.1-dev
fi

echo "=== cargo build novel-agent (src-tauri) ==="
cd "$ROOT"
cargo build -p novel-agent --manifest-path src-tauri/Cargo.toml
