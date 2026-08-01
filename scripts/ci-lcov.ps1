# Generate lcov.info for ci-crap (not part of the CRAP gate itself).
#
# Usage: .\scripts\ci-lcov.ps1
#
# Windows notes (see .claude/skills/cargo-crap SKILL.md 坑 3):
# - instrument-coverage 编译大 crate（novel-core 测试目标）在高并行下会 OOM（rustc
#   handle_alloc_error / 0xc0000409 崩溃，残留 invalid metadata）。固定 CARGO_BUILD_JOBS=2
#   并关闭增量编译（CARGO_INCREMENTAL=0）规避。
# - src-tauri（novel-agent）的 tauri-build 在 instrument-coverage 下不稳定，且已被
#   .cargo-crap.toml exclude（**/src-tauri/**）——用 --exclude novel-agent 跳过。
# - event_payload/dto 覆盖率需要 novel-server 的 tauri feature（--features tauri），
#   否则这些模块不编译、无覆盖数据（CRAP 假阳性）。

$ErrorActionPreference = "Stop"
Set-Location $PSScriptRoot\..

if (-not $env:NEXTEST_PROFILE) { $env:NEXTEST_PROFILE = "ci" }
if (-not $env:CARGO_BUILD_JOBS) { $env:CARGO_BUILD_JOBS = "2" }
if (-not $env:CARGO_INCREMENTAL) { $env:CARGO_INCREMENTAL = "0" }
$lcovPath = if ($env:LCOV_PATH) { $env:LCOV_PATH } else { "lcov.info" }

# llvm-cov prints informational lines to stderr; do not treat as terminating errors.
$prevEap = $ErrorActionPreference
$ErrorActionPreference = "Continue"
& cargo llvm-cov nextest --workspace --exclude novel-agent --features tauri --lcov --output-path $lcovPath
$code = $LASTEXITCODE
$ErrorActionPreference = $prevEap
exit $code
