[CmdletBinding()]
param()
$ErrorActionPreference = 'Stop'
$root = Split-Path -Parent $PSScriptRoot
$metadata = (& (Join-Path $PSScriptRoot 'cargo.ps1') -CargoArgs @('metadata','--locked','--offline','--format-version','1')) | ConvertFrom-Json
$licenseRoot = Join-Path $root 'licenses'
New-Item -ItemType Directory -Path $licenseRoot -Force | Out-Null
$inventory = @()
foreach ($package in $metadata.packages | Where-Object { $_.source }) {
    $packageRoot = Split-Path -Parent $package.manifest_path
    $texts = @(Get-ChildItem -LiteralPath $packageRoot -File | Where-Object Name -Match '^(LICENSE|COPYING|NOTICE)([-.].*)?$')
    if (-not $texts.Count) { throw "Missing license text for $($package.name)" }
    foreach ($text in $texts) { Copy-Item -LiteralPath $text.FullName -Destination (Join-Path $licenseRoot "$($package.name)-$($package.version)-$($text.Name)") -Force }
    $inventory += [ordered]@{name=$package.name;version=$package.version;license=$package.license;source=$package.source}
}
$inventory | ConvertTo-Json -Depth 4 | Set-Content -LiteralPath (Join-Path $licenseRoot 'dependencies.json') -Encoding utf8
$rustc = $null
if ($env:CARGO_HOME) {
    $candidate = Join-Path $env:CARGO_HOME 'bin/rustc.exe'
    if (Test-Path -LiteralPath $candidate -PathType Leaf) { $rustc = $candidate }
}
if (-not $rustc) { $rustc = (Get-Command rustc -ErrorAction Stop).Source }
$sysroot = (& $rustc --print sysroot).Trim()
if ($LASTEXITCODE -ne 0 -or -not $sysroot) { throw 'Could not locate the active Rust sysroot.' }
$runtimeNotices = Join-Path $sysroot 'share/doc/rust/COPYRIGHT-library.html'
if (-not (Test-Path -LiteralPath $runtimeNotices)) { throw 'Rust standard-library notices are missing. Install the matching rust-docs component before packaging.' }
Copy-Item -LiteralPath $runtimeNotices -Destination (Join-Path $licenseRoot 'rust-COPYRIGHT-library.html') -Force
foreach ($name in @('MIT.txt','Apache-2.0.txt','LLVM-exception.txt')) {
    Copy-Item -LiteralPath (Join-Path $sysroot ('share/doc/rust/licenses/' + $name)) -Destination (Join-Path $licenseRoot ('rust-' + $name)) -Force
}

