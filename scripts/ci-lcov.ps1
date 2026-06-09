# Generate lcov.info for ci-crap (not part of the CRAP gate itself).
#
# Usage: .\scripts\ci-lcov.ps1

$ErrorActionPreference = "Stop"
Set-Location $PSScriptRoot\..

if (-not $env:NEXTEST_PROFILE) { $env:NEXTEST_PROFILE = "ci" }
$lcovPath = if ($env:LCOV_PATH) { $env:LCOV_PATH } else { "lcov.info" }

# llvm-cov prints informational lines to stderr; do not treat as terminating errors.
$prevEap = $ErrorActionPreference
$ErrorActionPreference = "Continue"
& cargo llvm-cov nextest --workspace --all-features --lcov --output-path $lcovPath
$code = $LASTEXITCODE
$ErrorActionPreference = $prevEap
exit $code
