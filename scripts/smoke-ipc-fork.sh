#!/usr/bin/env bash
# Local smoke: IPC fork flood prevention (non-CI).
#
# Phase 1 — automated proxy (nextest + vitest; no WebView2).
# Phase 2 — optional log scan: bash scripts/smoke-ipc-fork.sh /path/to/dev-log.txt
# Phase 2 — manual: cargo tauri dev → parallel Fork, overlay closed → save log → re-run with $1
set -euo pipefail
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cd "$ROOT"

echo "=== IPC fork smoke ==="
echo ""

run_phase1() {
  echo "--- Phase 1: automated IPC proxy tests ---"
  # shellcheck source=ci-nextest-env.sh
  source "$ROOT/scripts/ci-nextest-env.sh"

  cargo nextest run -p novel-core \
    -E 'test(fork_stream) or test(interruptible) or test(fork_usage) or test(gated) or test(drain_subagent_jobs)' \
    --profile ci

  cargo nextest run -p novel-server --features tauri \
    -E 'test(stream_coalesce) or test(interruptible) or test(clear_removes)' \
    --profile ci

  (cd "$ROOT/ui" && npm test -- --run src/test/acceptance/ipc-flood-acceptance.test.ts)

  echo ""
  echo "Phase 1: PASSED"
  echo ""
}

scan_log() {
  local log_file="$1"
  if [ ! -f "$log_file" ]; then
    echo "ERROR: log file not found: $log_file" >&2
    exit 1
  fi
  echo "--- Phase 2: log scan ($log_file) ---"
  if command -v rg >/dev/null 2>&1; then
    if rg -q 'PostMessage failed|0x80070718' "$log_file"; then
      echo "FAIL: PostMessage quota errors found in log" >&2
      exit 1
    fi
    if rg -q 'engine_loop_exited' "$log_file"; then
      echo "WARN: engine_loop_exited found — investigate if unexpected"
    fi
  else
    if grep -qE 'PostMessage failed|0x80070718' "$log_file"; then
      echo "FAIL: PostMessage quota errors found in log" >&2
      exit 1
    fi
    if grep -q 'engine_loop_exited' "$log_file"; then
      echo "WARN: engine_loop_exited found — investigate if unexpected"
    fi
  fi
  echo "Phase 2 log scan: PASSED (no PostMessage / 0x80070718)"
}

print_manual_phase2() {
  echo "--- Phase 2: manual WebView2 smoke (PostMessage; not automatable here) ---"
  echo "1. cargo tauri dev"
  echo "2. Main session: trigger parallel ForkSubAgent (2 sub-agents), overlay CLOSED; wait drain"
  echo "3. (Optional) Open one overlay → live stream OK; close → stream stops"
  echo ""
  echo "Save dev terminal output, then re-run:"
  echo "  bash scripts/smoke-ipc-fork.sh /path/to/log.txt"
  echo "  .\\scripts\\smoke-ipc-fork.ps1 -LogFile C:\\path\\to\\log.txt"
  echo ""
  echo "Phase 2: SKIPPED (no log file). Phase 1 automated checks passed."
}

run_phase1

if [ -n "${1:-}" ]; then
  scan_log "$1"
else
  print_manual_phase2
fi
