#!/usr/bin/env bash
# Removes all work-level session databases under works/**/.novel-agent/.
set -euo pipefail
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
removed=0
while IFS= read -r dir; do
  for name in state.db state.db-wal state.db-shm state.db-journal; do
    f="$dir/$name"
    if [[ -f "$f" ]]; then
      rm -f "$f"
      echo "Removed $f"
      removed=$((removed + 1))
    fi
  done
done < <(find "$ROOT/works" -type d -name '.novel-agent' 2>/dev/null || true)
echo "Done. Removed $removed file(s). Restart the app and create a session if needed."
