# Removes all work-level session databases under works/**/.novel-agent/.
# Preserves knowledge/, chapters/, settings.json, and audit logs under .novel/logs/.
$ErrorActionPreference = "Stop"
$root = Split-Path -Parent $PSScriptRoot
$patterns = @(
    "state.db",
    "state.db-wal",
    "state.db-shm",
    "state.db-journal"
)
$removed = 0
Get-ChildItem -Path (Join-Path $root "works") -Recurse -Directory -Filter ".novel-agent" -ErrorAction SilentlyContinue |
    ForEach-Object {
        foreach ($name in $patterns) {
            Get-ChildItem -Path $_.FullName -Filter $name -File -ErrorAction SilentlyContinue |
                ForEach-Object {
                    Remove-Item -LiteralPath $_.FullName -Force
                    Write-Host "Removed $($_.FullName)"
                    $removed++
                }
        }
    }
Write-Host "Done. Removed $removed file(s). Restart the app and create a session if needed."
