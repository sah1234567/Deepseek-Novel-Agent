#!/usr/bin/env bash
# Fail fast when cargo/rustup are missing from PATH (common with WSL bash on Windows).
set -euo pipefail

if command -v cargo >/dev/null 2>&1; then
  return 0 2>/dev/null || exit 0
fi

echo "error: cargo not found in PATH" >&2

if [ -f /proc/version ] && grep -qiE 'microsoft|wsl' /proc/version 2>/dev/null; then
  cat >&2 <<'EOF'
hint: You are likely using WSL bash (C:\Windows\System32\bash.exe).
      Rust installed on Windows is not on WSL's PATH.

      Use Git for Windows bash instead (same as CI / ci-windows.ps1):
        & "${env:ProgramFiles}\Git\bin\bash.exe" scripts/ci-tauri-check.sh

      Or run the full Windows gate from PowerShell:
        .\scripts\ci-windows.ps1
EOF
else
  echo "hint: Install Rust (https://rustup.rs) and ensure cargo is on PATH." >&2
fi

return 1 2>/dev/null || exit 127
