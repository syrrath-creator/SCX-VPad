[CmdletBinding()]
param(
    [string]$WdkRoot = $env:SCX_WDK_ROOT,
    [string]$SdkRoot = ${env:ProgramFiles(x86)} + '\Windows Kits\10',
    [string]$SdkVersion = '10.0.26100.0',
    [switch]$AsObject
)
$ErrorActionPreference = 'Stop'
$repoRoot = Split-Path -Parent $PSScriptRoot
if (-not $WdkRoot) {
    $localCandidate = Join-Path $repoRoot '..\..\.tooling\wdk-nuget\10.0.26100.6584\c'
    $repoCandidate = Join-Path $repoRoot '.tooling\wdk\c'
    if (Test-Path -LiteralPath $localCandidate) { $WdkRoot = $localCandidate }
    elseif (Test-Path -LiteralPath $repoCandidate) { $WdkRoot = $repoCandidate }
    else { $WdkRoot = $SdkRoot }
}
$WdkRoot = [IO.Path]::GetFullPath($WdkRoot)
$vswhere = Join-Path ${env:ProgramFiles(x86)} 'Microsoft Visual Studio\Installer\vswhere.exe'
if (-not (Test-Path -LiteralPath $vswhere)) { throw 'Visual Studio Build Tools with C++ are required.' }
$vsRoot = (& $vswhere -latest -products '*' -requires Microsoft.VisualStudio.Component.VC.Tools.x86.x64 -property installationPath | Select-Object -First 1)
if (-not $vsRoot) { throw 'No Visual Studio C++ build tools found.' }
$msvc = Get-ChildItem -LiteralPath (Join-Path $vsRoot 'VC\Tools\MSVC') -Directory |
    Sort-Object Name -Descending | Select-Object -First 1
$result = [ordered]@{
    WdkRoot = $WdkRoot
    SdkRoot = $SdkRoot
    SdkVersion = $SdkVersion
    UmdfVersion = '2.25'
    MsvcRoot = $msvc.FullName
    Cl = Join-Path $msvc.FullName 'bin\Hostx64\x64\cl.exe'
    Link = Join-Path $msvc.FullName 'bin\Hostx64\x64\link.exe'
    Dumpbin = Join-Path $msvc.FullName 'bin\Hostx64\x64\dumpbin.exe'
    Rc = Join-Path $SdkRoot "bin\$SdkVersion\x64\rc.exe"
    InfVerif = Join-Path $WdkRoot "tools\$SdkVersion\x64\infverif.exe"
    Inf2Cat = Join-Path $WdkRoot "bin\$SdkVersion\x86\Inf2Cat.exe"
    WdfHeader = Join-Path $WdkRoot 'Include\wdf\umdf\2.25\wdf.h'
    WdfLibrary = Join-Path $WdkRoot 'Lib\wdf\umdf\x64\2.25\WdfDriverStubUm.lib'
    HidHeader = Join-Path $WdkRoot "Include\$SdkVersion\km\hidport.h"
}
foreach ($key in @('Cl','Link','Rc','InfVerif','Inf2Cat','WdfHeader','WdfLibrary','HidHeader')) {
    if (-not (Test-Path -LiteralPath $result[$key])) {
        throw "Missing $key at $($result[$key]). Set SCX_WDK_ROOT to the c/ directory of official Microsoft.Windows.WDK.x64 10.0.26100.6584 or an installed WDK. This script never installs tools or changes Windows."
    }
}
if ($AsObject) { [pscustomobject]$result } else { $result | ConvertTo-Json }
