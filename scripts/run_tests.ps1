# Novel Agent test script — matches GitHub CI (ci-rust-test.sh / --profile ci).

$ErrorActionPreference = "Stop"
Set-Location $PSScriptRoot\..

Write-Host "=== cargo nextest run --workspace --profile ci ===" -ForegroundColor Cyan
cargo nextest run --workspace --profile ci
if ($LASTEXITCODE -ne 0) { exit $LASTEXITCODE }

# Optional live DeepSeek API test (requires network + API key in env)
if ($env:DEEPSEEK_API_KEY) {
    Write-Host "=== Optional: live LLM smoke test ===" -ForegroundColor Yellow
    cargo nextest run -p novel-deepseek -E 'test(live_endpoints)' -- --nocapture
} else {
    Write-Host "Skipping live API test (set DEEPSEEK_API_KEY)" -ForegroundColor DarkGray
}

Write-Host "=== All tests passed ===" -ForegroundColor Green
