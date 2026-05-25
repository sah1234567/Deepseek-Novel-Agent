# Novel Agent full verification (Rust + frontend)

$ErrorActionPreference = "Stop"
Set-Location $PSScriptRoot\..

# Reduce parallel link jobs to avoid OOM on Windows CI/dev machines
$env:CARGO_BUILD_JOBS = "1"

function Invoke-Cargo {
    param([Parameter(Mandatory = $true)][string[]]$Args)
    $prev = $ErrorActionPreference
    $ErrorActionPreference = "Continue"
    & cargo @Args
    $code = $LASTEXITCODE
    $ErrorActionPreference = $prev
    if ($code -ne 0) { exit $code }
}

Write-Host "=== cargo fmt --all -- --check ===" -ForegroundColor Cyan
Invoke-Cargo @("fmt", "--all", "--", "--check")

Write-Host "=== cargo check --workspace ===" -ForegroundColor Cyan
Invoke-Cargo @("check", "--workspace")

Write-Host "=== cargo clippy (deny unwrap: core + knowledge + compaction + config + skills + llm) ===" -ForegroundColor Cyan
$strict = @(
    "novel-core",
    "novel-knowledge",
    "novel-compaction",
    "novel-config",
    "novel-skills",
    "novel-deepseek"
)
foreach ($pkg in $strict) {
    Invoke-Cargo @("clippy", "-p", $pkg, "--", "-D", "clippy::unwrap_used")
}

Write-Host "=== cargo nextest run --workspace ===" -ForegroundColor Cyan
Invoke-Cargo @("nextest", "run", "--workspace")

Write-Host "=== npm ci + build + test (ui/) ===" -ForegroundColor Cyan
Push-Location ui
try {
    npm ci
    if ($LASTEXITCODE -ne 0) { exit $LASTEXITCODE }
    npm run build
    if ($LASTEXITCODE -ne 0) { exit $LASTEXITCODE }
    npm test
    if ($LASTEXITCODE -ne 0) { exit $LASTEXITCODE }
} finally {
    Pop-Location
}

Write-Host ""
Write-Host "=== verify_all passed ===" -ForegroundColor Green
Write-Host ""
Write-Host "Manual smoke checklist (non-blocking):" -ForegroundColor Yellow
Write-Host "  [ ] cargo tauri dev -> send message, streaming, tool approval"
Write-Host "  [ ] resume session -> history loads"
Write-Host "  [ ] file tree shows chapters/knowledge"
Write-Host "  [ ] todo status click cycles pending/in_progress/completed"
Write-Host "  [ ] StatusBar shows token count after turn"
