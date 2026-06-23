# CRAP gate only: cargo crap --fail-above (reads existing lcov.info).
#
# Generate lcov first: .\scripts\ci-lcov.ps1
# Usage: .\scripts\ci-crap.ps1 [--summary | other cargo-crap flags...]

$ErrorActionPreference = "Stop"
Set-Location $PSScriptRoot\..

$threshold = if ($env:CRAP_THRESHOLD) { $env:CRAP_THRESHOLD } else { "20" }
$lcovPath = if ($env:LCOV_PATH) { $env:LCOV_PATH } else { "lcov.info" }

$prevEap = $ErrorActionPreference
$ErrorActionPreference = "Continue"
& cargo crap --lcov $lcovPath --workspace --fail-above --threshold $threshold @args
exit $LASTEXITCODE
