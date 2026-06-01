# Full verification — alias for ci-local.ps1 (Windows → ci-windows-gate / GitHub rust-windows).
# Usage: .\scripts\verify_all.ps1

$ErrorActionPreference = "Stop"
& "$PSScriptRoot/ci-local.ps1"
