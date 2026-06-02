#!/usr/bin/env bash
# Require Node.js >= 24 (matches ui/.nvmrc and package.json engines).
set -euo pipefail

_prepend_node24_if_needed() {
  local node_bin major

  if command -v node >/dev/null 2>&1; then
    major="$(node -p "process.versions.node.split('.')[0]")"
    if [ "$major" -ge 24 ]; then
      return 0
    fi
  fi

  # Git Bash on Windows: Cursor often prepends its own Node 22.
  for dir in /f/nodejs /c/Program\ Files/nodejs; do
    node_bin="$dir/node.exe"
    if [ -x "$node_bin" ]; then
      major="$("$node_bin" -p "process.versions.node.split('.')[0]")"
      if [ "$major" -ge 24 ]; then
        export PATH="$dir:$PATH"
        return 0
      fi
    fi
  done

  return 1
}

if ! _prepend_node24_if_needed; then
  echo "error: Node.js >= 24 required (see ui/.nvmrc)" >&2
  echo "hint: Install Node 24 LTS (https://nodejs.org/) and put it first in User PATH (before Cursor bundled Node 22)." >&2
  exit 1
fi

echo "=== node $(node -v) | npm $(npm -v) ==="
