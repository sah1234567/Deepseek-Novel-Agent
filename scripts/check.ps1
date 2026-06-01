# Novel Agent 编译与静态检查

$ErrorActionPreference = "Stop"
Set-Location $PSScriptRoot\..

Write-Host "=== cargo check --workspace ===" -ForegroundColor Cyan
cargo check --workspace
if ($LASTEXITCODE -ne 0) { exit $LASTEXITCODE }

Write-Host "=== cargo clippy --workspace --all-targets -- -D warnings -D clippy::unwrap_used ===" -ForegroundColor Cyan
cargo clippy --workspace --all-targets -- -D warnings -D clippy::unwrap_used
if ($LASTEXITCODE -ne 0) { exit $LASTEXITCODE }

Write-Host "=== check passed ===" -ForegroundColor Green
