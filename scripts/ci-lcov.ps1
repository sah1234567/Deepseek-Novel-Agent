# Generate lcov.info for ci-crap (not part of the CRAP gate itself).
#
# Usage: .\scripts\ci-lcov.ps1

$ErrorActionPreference = "Stop"
Set-Location $PSScriptRoot\..

if (-not $env:NEXTEST_PROFILE) { $env:NEXTEST_PROFILE = "ci" }
$lcovPath = if ($env:LCOV_PATH) { $env:LCOV_PATH } else { "lcov.info" }

& cargo llvm-cov nextest --workspace --all-features --lcov --output-path $lcovPath
exit $LASTEXITCODE
