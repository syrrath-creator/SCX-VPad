[CmdletBinding()]
param([string]$Destination, [switch]$SkipTests)
$ErrorActionPreference = 'Stop'
$root = Split-Path -Parent $PSScriptRoot
if (-not $Destination) { $Destination = Join-Path $root 'dist/SCX-VPad-0.1.0-windows-x64' }
$Destination = [IO.Path]::GetFullPath($Destination)
if (Test-Path -LiteralPath $Destination) { throw 'Choose a new package directory; existing packages are never overwritten.' }
if (-not $SkipTests) { & (Join-Path $PSScriptRoot 'cargo.ps1') -CargoArgs @('test','--workspace','--locked','--offline') }
& (Join-Path $PSScriptRoot 'cargo.ps1') -CargoArgs @('build','--release','--locked','--offline','-p','scx-vpad-service','-p','scx-vpadctl')
# Keep the driver's no_std build separate from the SDK's std-enabled core.
& (Join-Path $PSScriptRoot 'cargo.ps1') -CargoArgs @('build','--release','--locked','--offline','-p','scx-vpad-core','--no-default-features','--target-dir','target/driver-core')
& (Join-Path $PSScriptRoot 'build-driver.ps1') -RustLibrary (Join-Path $root 'target/driver-core/release/scx_vpad_core.lib')
& (Join-Path $PSScriptRoot 'collect-licenses.ps1')
$files = [ordered]@{
    'bin/scx-vpad-service.exe' = 'target/release/scx-vpad-service.exe'
    'bin/scx-vpadctl.exe' = 'target/release/scx-vpadctl.exe'
    'bin/xinput_probe.exe' = 'target/driver/x64/xinput_probe.exe'
    'driver/ScxVPad.dll' = 'target/driver/x64/ScxVPad.dll'
    'driver/scxvpad.inf' = 'target/driver/x64/scxvpad.inf'
    'driver/scxxusb.inf' = 'target/driver/x64/scxxusb.inf'
    'driver/scxvpad.cat' = 'target/driver/x64/scxvpad.cat'
    'installer/Install.ps1' = 'installer/Install.ps1'
    'installer/Uninstall.ps1' = 'installer/Uninstall.ps1'
    'installer/GameInput.ps1' = 'installer/GameInput.ps1'
    'LICENSE' = 'LICENSE'
    'README.md' = 'README.md'
    'SECURITY.md' = 'SECURITY.md'
    'SDK.md' = 'SDK.md'
    'crates/scx-vpad-client/examples/xbox_press.rs' = 'crates/scx-vpad-client/examples/xbox_press.rs'
    'VALIDATION.md' = 'VALIDATION.md'
    'RELEASE-NOTES.md' = 'RELEASE-NOTES.md'
    'crates/scx-vpad-core/PROVENANCE.md' = 'crates/scx-vpad-core/PROVENANCE.md'
    'crates/scx-vpad-core/XUSB-CONTRACT.md' = 'crates/scx-vpad-core/XUSB-CONTRACT.md'
    'driver/README.md' = 'driver/README.md'
    'THIRD-PARTY-NOTICES.txt' = 'THIRD-PARTY-NOTICES.txt'
}
Get-ChildItem -LiteralPath (Join-Path $root 'licenses') -File | ForEach-Object { $files['licenses/' + $_.Name] = 'licenses/' + $_.Name }
$manifest = @()
foreach ($item in $files.GetEnumerator()) {
    $source = Join-Path $root $item.Value
    if (-not (Test-Path -LiteralPath $source -PathType Leaf)) { throw "Required package file missing: $source" }
    $target = Join-Path $Destination $item.Key
    New-Item -ItemType Directory -Path (Split-Path -Parent $target) -Force | Out-Null
    Copy-Item -LiteralPath $source -Destination $target
    $manifest += [ordered]@{ path=$item.Key; bytes=(Get-Item -LiteralPath $target).Length; sha256=(Get-FileHash -LiteralPath $target -Algorithm SHA256).Hash }
}
[ordered]@{ product='SCX-VPad'; version='0.1.0'; driverVersion='0.1.0.28'; platform='windows-x64'; minimumBuild=22000; files=$manifest } |
    ConvertTo-Json -Depth 5 | Set-Content -LiteralPath (Join-Path $Destination 'package-manifest.json') -Encoding utf8
Write-Output "Packaged unsigned development binaries: $Destination"
Write-Output 'The package is signed locally by its explicit administrative installer. No private key is distributed.'

