# Tauri icons

Compile-time requirements (`tauri-build` / `generate_context!`):

| File | Platform |
|------|----------|
| `icon.png` | Linux, **macOS** (`generate_context!`; macOS dev tries `.icns` first, then `.png`) |
| `icon.ico` | **Windows** (`.rc` resource file in `tauri-build`) |

`bundle.icon` in `tauri.conf.json` lists both for installers (macOS bundler can build `.icns` from png/ico).

Replace placeholders with production assets:

```bash
cargo tauri icon path/to/source-1024.png
```

CI ensures placeholders via `scripts/ci-tauri-icons.sh`; macOS job runs `scripts/ci-tauri-check.sh`.
