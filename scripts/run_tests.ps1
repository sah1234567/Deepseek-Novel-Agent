# Novel Agent test script — same as GitHub CI (scripts/ci-rust-test.sh / --profile ci).

$ErrorActionPreference = "Stop"
Set-Location $PSScriptRoot\..

function Resolve-Bash {
    $candidates = @(
        "${env:ProgramFiles}\Git\bin\bash.exe",
        "${env:ProgramFiles(x86)}\Git\bin\bash.exe"
    )
    foreach ($c in $candidates) {
        if (Test-Path $c) { return $c }
    }
    $bash = Get-Command bash -ErrorAction SilentlyContinue
    if ($bash -and $bash.Source -notlike "*\Windows\System32\bash.exe") {
        return $bash.Source
    }
    return $null
}

$bashPath = Resolve-Bash
if (-not $bashPath) {
    Write-Error "Git Bash required. Install Git for Windows, then re-run .\scripts\run_tests.ps1"
    exit 1
}

Write-Host "=== run_tests: ci-rust-test.sh via $bashPath ===" -ForegroundColor Cyan
& $bashPath "$PSScriptRoot/ci-rust-test.sh"
if ($LASTEXITCODE -ne 0) { exit $LASTEXITCODE }

if ($env:DEEPSEEK_API_KEY) {
    Write-Host "=== Optional: live LLM smoke test ===" -ForegroundColor Yellow
    cargo nextest run -p novel-deepseek -E 'test(live_endpoints)' -- --nocapture
} else {
    Write-Host "Skipping live API test (set DEEPSEEK_API_KEY)" -ForegroundColor DarkGray
}

Write-Host "=== All tests passed ===" -ForegroundColor Green
