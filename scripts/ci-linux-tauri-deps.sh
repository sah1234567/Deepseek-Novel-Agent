#!/usr/bin/env bash
# WebKit/GTK dev packages for Tauri on GitHub ubuntu-latest (no-op elsewhere).
set -euo pipefail

if [ "${GITHUB_ACTIONS:-}" != "true" ] || [ "$(uname -s)" != "Linux" ]; then
  exit 0
fi

echo "=== Install Linux Tauri / WebKit dependencies ==="
sudo apt-get update
sudo apt-get install -y \
  libwebkit2gtk-4.1-dev libgtk-3-dev libayatana-appindicator3-dev librsvg2-dev \
  libssl-dev libsoup-3.0-dev libjavascriptcoregtk-4.1-dev
