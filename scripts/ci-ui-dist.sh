#!/usr/bin/env bash
# Build ui/dist for Tauri generate_context! (required before workspace check/clippy on src-tauri).
# Skips when dist already exists (e.g. ci-gate-core after ci-frontend). Set CI_UI_DIST_FORCE=1 to rebuild.
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
bash "$ROOT/scripts/ci-check-node.sh"
cd "$ROOT/ui"

if [ -f dist/index.html ] && [ "${CI_UI_DIST_FORCE:-}" != "1" ]; then
  echo "=== ui/dist present, skip npm build (CI_UI_DIST_FORCE=1 to rebuild) ==="
  exit 0
fi

echo "=== npm ci (ui) ==="
npm ci

echo "=== npm run build (ui/dist for Tauri) ==="
npm run build

if [ ! -f dist/index.html ]; then
  echo "::error::ui/dist missing after npm run build"
  exit 1
fi
