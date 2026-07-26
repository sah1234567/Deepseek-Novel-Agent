# Windows CI gate — same checks as GitHub job "Rust (Windows gate)" in ci.yml.
# Usage: .\scripts\ci-windows.ps1
#
# Requires Git Bash so Windows cargo/pnpm stay on PATH (avoid WSL bash).

$ErrorActionPreference = "Stop"
Set-Location $PSScriptRoot\..

# GHA sets CARGO_BUILD_JOBS=1 for runner RAM. Local: leave unset → use all CPU cores.
# Force single-job locally only if you set it yourself: $env:CARGO_BUILD_JOBS = "1"
$env:CARGO_TERM_COLOR = "always"
$env:RUST_BACKTRACE = "1"
if ($env:CARGO_BUILD_JOBS) {
    Write-Host "=== CARGO_BUILD_JOBS=$($env:CARGO_BUILD_JOBS) ===" -ForegroundColor DarkGray
} else {
    Write-Host "=== CARGO_BUILD_JOBS=unset (cargo default parallelism) ===" -ForegroundColor DarkGray
}

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
bash not found. Install Git for Windows (recommended).
Then re-run: .\scripts\ci-windows.ps1
"@
    exit 1
}

Write-Host "=== ci-windows: running scripts/ci-windows-gate.sh via $bashPath ===" -ForegroundColor Cyan
# Native stderr (pnpm/cargo progress) must not trip Stop; only $LASTEXITCODE matters.
$prevEap = $ErrorActionPreference
$ErrorActionPreference = "Continue"
try {
    & $bashPath "$PSScriptRoot/ci-windows-gate.sh"
    $code = $LASTEXITCODE
} finally {
    $ErrorActionPreference = $prevEap
}
if ($code -ne 0) { exit $code }

Write-Host ""
Write-Host "=== ci-windows passed ===" -ForegroundColor Green
