#!/usr/bin/env bash
# Frontend CI gate: npm test (fail on toolchain warnings) + build.
# Aligns with post-change-checklist step 6 (Node 24+ via ci-check-node.sh).
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
bash "$ROOT/scripts/ci-check-node.sh"
cd "$ROOT/ui"

npm ci

echo "=== npm audit (critical) ==="
npm audit --audit-level=critical

echo "=== npm test ==="
npm test 2>&1 | tee /tmp/vitest.log
if grep -qE '\bDEPRECATED\b|duplicate case in| is deprecated|Warning \((vitest|vite|esbuild)' /tmp/vitest.log; then
  echo "::error::Vitest output contains warnings (see post-change-checklist step 6)"
  grep -E '\bDEPRECATED\b|duplicate case in| is deprecated|Warning \(' /tmp/vitest.log || true
  exit 1
fi

echo "=== npm run build ==="
npm run build
