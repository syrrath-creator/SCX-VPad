[CmdletBinding()]
param()
$ErrorActionPreference = 'Stop'
$root = Split-Path -Parent $PSScriptRoot
$version = '10.0.26100.6584'
$expectedHash = 'C393D03DFB640B5C92F546B32F6770EF68CD3AAF691956E7D66D8E2C28A1B55E'
$toolsRoot = Join-Path $root '.tooling'
$destination = Join-Path $toolsRoot 'wdk'
if (Test-Path -LiteralPath $destination) { throw 'Local WDK directory already exists. Set SCX_WDK_ROOT to reuse a verified installation.' }
New-Item -ItemType Directory -Path $toolsRoot -Force | Out-Null
$archive = Join-Path $toolsRoot "Microsoft.Windows.WDK.x64.$version.zip"
$url = "https://api.nuget.org/v3-flatcontainer/microsoft.windows.wdk.x64/$version/microsoft.windows.wdk.x64.$version.nupkg"
Invoke-WebRequest -Uri $url -OutFile $archive
if ((Get-FileHash -LiteralPath $archive -Algorithm SHA256).Hash -ne $expectedHash) { throw 'Official WDK package did not match the pinned SHA256. Nothing was extracted.' }
Expand-Archive -LiteralPath $archive -DestinationPath $destination
$env:SCX_WDK_ROOT = Join-Path $destination 'c'
& (Join-Path $PSScriptRoot 'check-wdk.ps1') -WdkRoot $env:SCX_WDK_ROOT

