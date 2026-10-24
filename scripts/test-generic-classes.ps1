[CmdletBinding()]
param([string]$ControlTool)
$ErrorActionPreference = 'Stop'
$root = Split-Path -Parent $PSScriptRoot
if (-not $ControlTool) { $ControlTool = Join-Path $root 'target/release/scx-vpadctl.exe' }
$ControlTool = (Resolve-Path -LiteralPath $ControlTool).Path
Write-Output 'Explicit Windows .33 test: creates owned synthetic HID input. Close games before running. No installation/elevation.'
& $ControlTool self-test-generic-classes
if ($LASTEXITCODE -ne 0) { throw 'Generic-class Windows semantic input/lifecycle test failed.' }
foreach ($profile in @('wheel','flight','arcade')) {
    & $ControlTool security-check $profile
    if ($LASTEXITCODE -ne 0) { throw "Security test failed for $profile." }
}
Write-Output 'PASS: Windows generic classes and per-profile access/capability isolation. All test leases have been released.'
