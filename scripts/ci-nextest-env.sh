#!/usr/bin/env bash
# Shared nextest parallelism for CI (source from ci-rust-test.sh; do not execute directly).
# Aligns Windows/macOS/Linux with GHA Ubuntu (multi-core parallel test runs).

ci_detect_test_threads() {
  if [ -n "${NEXTEST_TEST_THREADS:-}" ]; then
    echo "$NEXTEST_TEST_THREADS"
    return 0
  fi
  if command -v nproc >/dev/null 2>&1; then
    nproc
    return 0
  fi
  case "$(uname -s)" in
    Darwin)
      sysctl -n hw.logicalcpu 2>/dev/null || echo 8
      ;;
    MINGW* | MSYS* | CYGWIN*)
      echo "${NUMBER_OF_PROCESSORS:-8}"
      ;;
    *)
      echo 8
      ;;
  esac
}

export NEXTEST_TEST_THREADS="$(ci_detect_test_threads)"
echo "=== nextest: NEXTEST_TEST_THREADS=$NEXTEST_TEST_THREADS ==="
