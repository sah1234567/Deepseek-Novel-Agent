#!/usr/bin/env bash
# Build ui/dist for Tauri generate_context! (required before workspace check/clippy on src-tauri).
# Skips when dist already exists (e.g. ci-gate-core after ci-frontend). Set CI_UI_DIST_FORCE=1 to rebuild.
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
bash "$ROOT/scripts/ci-check-node.sh"
cd "$ROOT/ui"

if [ -f dist/index.html ] && [ "${CI_UI_DIST_FORCE:-}" != "1" ]; then
  echo "=== ui/dist present, skip pnpm build (CI_UI_DIST_FORCE=1 to rebuild) ==="
  exit 0
fi

echo "=== pnpm install --frozen-lockfile (ui) ==="
pnpm install --frozen-lockfile

echo "=== pnpm run build (ui/dist for Tauri) ==="
pnpm run build

if [ ! -f dist/index.html ]; then
  echo "::error::ui/dist missing after pnpm run build"
  exit 1
fi
