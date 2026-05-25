# Novel Agent 编译与静态检查

$ErrorActionPreference = "Stop"
Set-Location $PSScriptRoot\..

Write-Host "=== cargo check --workspace ===" -ForegroundColor Cyan
cargo check --workspace
if ($LASTEXITCODE -ne 0) { exit $LASTEXITCODE }

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
    cargo clippy -p $pkg -- -D clippy::unwrap_used
    if ($LASTEXITCODE -ne 0) { exit $LASTEXITCODE }
}

Write-Host "=== check passed ===" -ForegroundColor Green
