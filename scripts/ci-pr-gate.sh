#!/usr/bin/env bash
# PR-level CI gate — mirrors .github/workflows/ci.yml Ubuntu jobs (frontend + rust + tauri + audit).
# Used by: local WSL/macOS/Linux bash. On Windows use ci-windows-gate.sh (ci-local.ps1 / ci-windows.ps1).
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
bash "$ROOT/scripts/ci-gate-core.sh"

echo ""
echo "=== ci-pr-gate passed ==="
