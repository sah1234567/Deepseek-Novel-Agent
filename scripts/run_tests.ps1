# Novel Agent 测试脚本
# 使用 cargo-nextest 替代 cargo test（更快、自带超时检测）

$ErrorActionPreference = "Stop"
Set-Location $PSScriptRoot\..

Write-Host "=== Novel Agent: cargo nextest run (workspace) ===" -ForegroundColor Cyan
cargo nextest run --workspace
if ($LASTEXITCODE -ne 0) { exit $LASTEXITCODE }

Write-Host "=== Novel Agent: cargo nextest run (integration) ===" -ForegroundColor Cyan
cargo nextest run -p novel-integration-tests
if ($LASTEXITCODE -ne 0) { exit $LASTEXITCODE }

# Optional live DeepSeek API test (requires network + API key in env)
if ($env:DEEPSEEK_API_KEY) {
    Write-Host "=== Optional: live LLM smoke test ===" -ForegroundColor Yellow
    cargo nextest run -p novel-deepseek -E 'test(live_endpoints)' -- --nocapture
} else {
    Write-Host "Skipping live API test (set DEEPSEEK_API_KEY)" -ForegroundColor DarkGray
}

Write-Host "=== All tests passed ===" -ForegroundColor Green
