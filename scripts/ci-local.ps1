# Local CI gate.
# - Windows: scripts/ci-windows-gate.sh (same as GitHub rust-windows job)
# - Other OS: scripts/ci-pr-gate.sh (same as GitHub Ubuntu PR jobs combined)
# Usage: .\scripts\ci-local.ps1
#
# On Windows, prefer Git Bash (see ci-windows.ps1). WSL bash lacks cargo unless installed in WSL.

$ErrorActionPreference = "Stop"
Set-Location $PSScriptRoot\..

$env:CARGO_BUILD_JOBS = "1"
$env:CARGO_TERM_COLOR = "always"
$env:RUST_BACKTRACE = "1"

function Resolve-Bash {
    $gitBashCandidates = @(
        "${env:ProgramFiles}\Git\bin\bash.exe",
        "${env:ProgramFiles(x86)}\Git\bin\bash.exe"
    )
    foreach ($candidate in $gitBashCandidates) {
        if (Test-Path $candidate) {
            return $candidate
        }
    }

    $bash = Get-Command bash -ErrorAction SilentlyContinue
    if (-not $bash) {
        return $null
    }

    if ($bash.Source -like "*\Windows\System32\bash.exe") {
        Write-Warning "Only WSL bash found; Windows cargo may be unavailable. Install Git for Windows."
    }
    return $bash.Source
}

$bashPath = Resolve-Bash
if (-not $bashPath) {
    Write-Error @"
bash not found. Install Git for Windows (recommended) or WSL with Rust toolchain.
Then re-run: .\scripts\ci-local.ps1
"@
    exit 1
}

$isWindows = ($IsWindows -or $env:OS -match "Windows")
if ($isWindows) {
    $gateScript = "ci-windows-gate.sh"
    $gateLabel = "ci-windows-gate (GitHub rust-windows)"
} else {
    $gateScript = "ci-pr-gate.sh"
    $gateLabel = "ci-pr-gate (GitHub Ubuntu PR jobs)"
}

Write-Host "=== ci-local: running scripts/$gateScript via $bashPath ===" -ForegroundColor Cyan
Write-Host "    ($gateLabel)" -ForegroundColor DarkGray
& $bashPath "$PSScriptRoot/$gateScript"
if ($LASTEXITCODE -ne 0) { exit $LASTEXITCODE }

Write-Host ""
Write-Host "=== ci-local passed ===" -ForegroundColor Green
