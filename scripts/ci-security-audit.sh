#!/usr/bin/env bash
# Dependency security audit (matches ci.yml security-audit job / rustsec/audit-check).
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cd "$ROOT"

AUDIT_VERSION="${CARGO_AUDIT_VERSION:-0.22.1}"

if [ "${SKIP_SECURITY_AUDIT:-}" = "1" ]; then
  echo "=== cargo audit: skipped (SKIP_SECURITY_AUDIT=1) ==="
  exit 0
fi

audit_db_dir() {
  if [ -n "${CARGO_AUDIT_DB:-}" ]; then
    echo "$CARGO_AUDIT_DB"
  else
    echo "${CARGO_HOME:-$HOME/.cargo}/advisory-db"
  fi
}

audit_db_cached() {
  local db
  db="$(audit_db_dir)"
  [ -d "$db/.git" ] || [ -d "$db" ] && [ -n "$(ls -A "$db" 2>/dev/null || true)" ]
}

# Avoid dead local git/http proxies during install and advisory-db fetch.
run_without_proxy() {
  local empty_git_config
  empty_git_config="$(mktemp)"
  : >"$empty_git_config"
  CARGO_NET_GIT_FETCH_WITH_CLI=false \
    HTTP_PROXY= HTTPS_PROXY= ALL_PROXY= http_proxy= https_proxy= all_proxy= \
    GIT_CONFIG_GLOBAL="$empty_git_config" \
    GIT_CONFIG_COUNT=2 \
    GIT_CONFIG_KEY_0=http.proxy \
    GIT_CONFIG_VALUE_0= \
    GIT_CONFIG_KEY_1=https.proxy \
    GIT_CONFIG_VALUE_1= \
    "$@"
  local code=$?
  rm -f "$empty_git_config"
  return "$code"
}

audit_version_ok() {
  command -v cargo-audit >/dev/null 2>&1 || return 1
  cargo audit --version 2>/dev/null | grep -q "$AUDIT_VERSION"
}

install_audit_cargo() {
  echo "=== installing cargo-audit $AUDIT_VERSION (cargo install) ==="
  run_without_proxy cargo install cargo-audit --locked --version "$AUDIT_VERSION"
}

install_audit_windows_binary() {
  local tmp zip exe
  tmp="$(mktemp -d)"
  zip="$tmp/cargo-audit.zip"
  exe="$tmp/cargo-audit.exe"
  local url="https://github.com/rustsec/rustsec/releases/download/cargo-audit%2Fv${AUDIT_VERSION}/cargo-audit-x86_64-pc-windows-msvc-v${AUDIT_VERSION}.zip"

  echo "=== installing cargo-audit $AUDIT_VERSION (GitHub release) ==="
  if command -v curl >/dev/null 2>&1; then
    run_without_proxy curl -fsSL --noproxy '*' -o "$zip" "$url"
  elif command -v wget >/dev/null 2>&1; then
    run_without_proxy wget -q -O "$zip" "$url"
  else
    echo "curl or wget required to download cargo-audit on Windows"
    return 1
  fi

  if command -v unzip >/dev/null 2>&1; then
    unzip -q -o "$zip" -d "$tmp"
  elif command -v tar >/dev/null 2>&1; then
    run_without_proxy tar -xf "$zip" -C "$tmp"
  else
    echo "unzip or tar required to extract cargo-audit archive"
    return 1
  fi

  exe="$(find "$tmp" -name 'cargo-audit.exe' -print -quit)"
  if [ -z "$exe" ]; then
    echo "cargo-audit.exe not found in release archive"
    return 1
  fi

  mkdir -p "${CARGO_HOME:-$HOME/.cargo}/bin"
  cp "$exe" "${CARGO_HOME:-$HOME/.cargo}/bin/cargo-audit.exe"
  rm -rf "$tmp"
}

run_cargo_audit() {
  if [ "${1:-}" = "--no-fetch" ]; then
    cargo audit --no-fetch
  else
    cargo audit
  fi
}

print_audit_network_hint() {
  cat >&2 <<'EOF'
cargo audit failed to reach GitHub (RustSec advisory-db).

Common causes:
  - No VPN / proxy to github.com (e.g. mainland network)
  - Broken system HTTP proxy (try unsetting HTTP_PROXY / HTTPS_PROXY)
  - Corporate firewall blocking git HTTPS

Options:
  1. Fix network, then re-run: bash scripts/ci-security-audit.sh
  2. If you already have ~/.cargo/advisory-db, script will retry with --no-fetch
  3. Local only — skip this step: SKIP_SECURITY_AUDIT=1 .\scripts\ci-windows.ps1
  4. Pre-fetch once when online: cargo audit   (then CI can use cached DB)
EOF
}

if ! audit_version_ok; then
  if ! install_audit_cargo; then
    case "$(uname -s)" in
      MINGW* | MSYS* | CYGWIN*)
        install_audit_windows_binary
        ;;
      *)
        echo "cargo-audit install failed; fix network/proxy or pre-install cargo-audit $AUDIT_VERSION"
        exit 1
        ;;
    esac
  fi
fi

if ! audit_version_ok; then
  echo "cargo-audit $AUDIT_VERSION not available after install"
  exit 1
fi

echo "=== cargo audit ==="

# 1) Respect user proxy / default git (needed in some regions).
if run_cargo_audit; then
  exit 0
fi

echo "=== cargo audit: retry without proxy (stale proxy workaround) ==="
if run_without_proxy run_cargo_audit; then
  exit 0
fi

# 2) Offline-ish: use cached advisory DB if present.
if audit_db_cached; then
  echo "=== cargo audit: using cached advisory-db (--no-fetch) ==="
  if run_cargo_audit --no-fetch; then
    echo "warning: advisory-db was not updated (network); audit used local cache" >&2
    exit 0
  fi
fi

print_audit_network_hint
exit 1
