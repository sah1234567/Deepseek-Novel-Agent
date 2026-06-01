#!/usr/bin/env bash
# Windows CI gate — canonical entry for local Windows and GitHub windows-latest.
# Same steps as ci-gate-core.sh; kept as a named contract so local/GitHub stay aligned.
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
bash "$ROOT/scripts/ci-gate-core.sh"

echo ""
echo "=== ci-windows-gate passed ==="
