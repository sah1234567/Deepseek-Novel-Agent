#!/usr/bin/env bash
# Cross-platform Tauri shell compile check (ui/dist + icons + cargo check novel-agent).
# Used on macOS CI and locally; Ubuntu/Windows full gates also cover this via workspace check/build.
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cd "$ROOT"

# shellcheck source=ci-require-toolchain.sh
source "$ROOT/scripts/ci-require-toolchain.sh"

bash "$ROOT/scripts/ci-ui-dist.sh"
bash "$ROOT/scripts/ci-tauri-icons.sh"

echo "=== cargo check -p novel-agent (src-tauri) ==="
cargo check -p novel-agent --manifest-path src-tauri/Cargo.toml
