#!/usr/bin/env bash
# Ensure Tauri compile-time icons exist:
#   - icons/icon.png  (generate_context! on Linux + macOS; macOS dev may prefer .icns but falls back)
#   - icons/icon.ico  (Windows resource file in tauri-build)
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
ICONS="$ROOT/src-tauri/icons"
PNG="$ICONS/icon.png"
ICO="$ICONS/icon.ico"

mkdir -p "$ICONS"

if [ -f "$PNG" ] && [ -f "$ICO" ]; then
  echo "=== tauri icons: ok ($PNG, $ICO) ==="
  exit 0
fi

PYTHON=""
for cmd in python3 python; do
  if command -v "$cmd" >/dev/null 2>&1; then
    PYTHON="$cmd"
    break
  fi
done
if [ -z "$PYTHON" ]; then
  echo "error: python3/python required to generate missing Tauri icons" >&2
  exit 1
fi

echo "=== tauri icons: ensuring placeholder png + ico ==="
"$PYTHON" - "$PNG" "$ICO" <<'PY'
import base64
import struct
import sys
from pathlib import Path

png_path, ico_path = Path(sys.argv[1]), Path(sys.argv[2])

png_b64 = (
    "iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAYAAAAfFcSJAAAADUlEQVR42mP8z8BQDwAEhQGAhKmMIQAAAABJRU5ErkJggg=="
)
if not png_path.is_file():
    png_path.write_bytes(base64.b64decode(png_b64))
    print(f"wrote {png_path}")

if ico_path.is_file():
    sys.exit(0)

width = height = 16
bmp_header_size = 40
xor_size = width * height * 4
and_row_bytes = ((width + 31) // 32) * 4
and_size = and_row_bytes * height
image_size = bmp_header_size + xor_size + and_size
offset = 6 + 16

header = struct.pack("<HHH", 0, 1, 1)
entry = struct.pack(
    "<BBBBHHII",
    width,
    height,
    0,
    0,
    1,
    32,
    image_size,
    offset,
)
bmp = bytearray()
bmp += struct.pack(
    "<IIIHHIIIIII",
    40,
    width,
    height * 2,
    1,
    32,
    0,
    xor_size + and_size,
    0,
    0,
    0,
    0,
)
# BGRA indigo (#6366f1)
bmp += bytes([241, 102, 99, 255]) * (width * height)
bmp += bytes(and_size)

ico_path.write_bytes(header + entry + bmp)
print(f"wrote {ico_path}")
PY

echo "=== tauri icons: done ==="
