#!/usr/bin/env bash
# Dependency security audit (matches ci.yml security-audit job / rustsec/audit-check).
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cd "$ROOT"

AUDIT_VERSION="${CARGO_AUDIT_VERSION:-0.22.1}"

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
    # Git for Windows unzip fallback
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
run_without_proxy cargo audit
