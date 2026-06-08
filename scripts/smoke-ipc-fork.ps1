# Local IPC fork smoke (non-CI).
# Usage:
#   .\scripts\smoke-ipc-fork.ps1
#   .\scripts\smoke-ipc-fork.ps1 -LogFile C:\path\to\tauri-dev-log.txt
#
# Phase 1: nextest + vitest (automated). Phase 2: optional log grep or manual tauri dev.

param(
    [string]$LogFile = ""
)

$ErrorActionPreference = "Stop"
Set-Location $PSScriptRoot\..

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
        Write-Warning "Only WSL bash found; install Git for Windows or run: bash scripts/smoke-ipc-fork.sh"
    }
    return $bash.Source
}

$bashPath = Resolve-Bash
if (-not $bashPath) {
    Write-Error "bash not found. Install Git for Windows, then re-run: .\scripts\smoke-ipc-fork.ps1"
    exit 1
}

$sh = Join-Path $PSScriptRoot "smoke-ipc-fork.sh"
if ($LogFile) {
    & $bashPath $sh $LogFile
} else {
    & $bashPath $sh
}
if ($LASTEXITCODE -ne 0) { exit $LASTEXITCODE }

Write-Host ""
Write-Host "=== smoke-ipc-fork finished (exit 0) ===" -ForegroundColor Green
