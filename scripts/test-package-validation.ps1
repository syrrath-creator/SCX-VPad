[CmdletBinding()]
param([switch]$RequireWindowsPowerShell51)
$ErrorActionPreference = 'Stop'
Set-StrictMode -Version Latest
if ($RequireWindowsPowerShell51 -and ($PSVersionTable.PSEdition -ne 'Desktop' -or $PSVersionTable.PSVersion.Major -ne 5 -or $PSVersionTable.PSVersion.Minor -ne 1)) { throw 'Run this test in Windows PowerShell 5.1.' }
. (Join-Path (Split-Path -Parent $PSScriptRoot) 'installer/Package.ps1')
$tempParent = [IO.Path]::GetFullPath([IO.Path]::GetTempPath()).TrimEnd('\')
$testName = 'SCX-VPad-package-test-' + [Guid]::NewGuid().ToString('N')
$testRoot = Join-Path $tempParent $testName
$script:checks = 0
function Write-Manifest($Manifest) { ConvertTo-Json -InputObject $Manifest -Depth 7 | Set-Content -LiteralPath (Join-Path $testRoot 'package-manifest.json') -Encoding utf8 }
function Valid { Write-Manifest $script:baseline; Get-ScxVerifiedPackage $testRoot | Out-Null; $script:checks++ }
function Rejected([scriptblock]$Mutation, [string]$Label) {
    $copy = ConvertFrom-Json (ConvertTo-Json -InputObject $script:baseline -Depth 7)
    & $Mutation $copy
    Write-Manifest $copy
    $rejected = $false
    try { Get-ScxVerifiedPackage $testRoot | Out-Null } catch { $rejected = $true }
    if (-not $rejected) { throw ('FAIL: ' + $Label) }
    $script:checks++
    Write-Output ('PASS: ' + $Label)
}
try {
    $members = @('installer/Install.ps1','installer/Uninstall.ps1','installer/GameInput.ps1','installer/Service.ps1','installer/Package.ps1','driver/ScxVPad.dll','driver/scxvpad.inf','driver/scxxusb.inf','driver/scxvpad.cat','bin/scx-vpad-service.exe','bin/scx-vpadctl.exe','bin/xinput_probe.exe','LICENSE','THIRD-PARTY-NOTICES.txt','driver/ScxSource.dll','driver/scxsource.inf','driver/scxsource.cat')
    $files = foreach ($relative in $members) {
        $path = Join-Path $testRoot $relative
        New-Item -ItemType Directory -Path (Split-Path -Parent $path) -Force | Out-Null
        [IO.File]::WriteAllText($path, 'Synthetic test member ' + $relative)
        [pscustomobject]@{path=$relative; bytes=(Get-Item -LiteralPath $path).Length; sha256=(Get-FileHash -LiteralPath $path -Algorithm SHA256).Hash}
    }
    $script:baseline = [pscustomobject]@{product='SCX-VPad'; version='0.1.0'; driverVersion='0.1.0.35'; releaseChannel='stable'; platform='windows-x64'; minimumBuild=22000; files=@($files)}
    Valid
    Rejected {param($m) $m.platform='windows-arm64'} 'reject wrong architecture'
    Rejected {param($m) $m.driverVersion="0.1.0.35`n"} 'reject malformed version'
    Rejected {param($m) $m.minimumBuild=19041} 'reject incompatible platform declaration'
    Rejected {param($m) $m.releaseChannel='unverified'} 'reject unknown release channel'
    Rejected {param($m) $m.files=@($m.files | Where-Object path -ne 'driver/scxvpad.cat')} 'reject missing driver catalogue'
    Rejected {param($m) $m.files=@($m.files | Where-Object path -ne 'installer/Package.ps1')} 'reject missing verifier'
    Rejected {param($m) $m.files=@($m.files | Where-Object path -ne 'driver/scxsource.cat')} 'reject partial Source Guard payload'
    Rejected {param($m) $m.files=@($m.files | Where-Object path -notlike 'driver/*Source*')} 'reject Stable without Source Guard'
    Rejected {param($m) $m.files[0].bytes++} 'reject length mismatch'
    Rejected {param($m) $m.files[0].bytes=-1} 'reject negative length'
    Rejected {param($m) $m.files[0].bytes='12'} 'reject non-integer length'
    Rejected {param($m) $m.files[0].bytes=33554433} 'reject oversized member'
    Rejected {param($m) $m.files[0].sha256='0'*64} 'reject content hash mismatch'
    Rejected {param($m) $m.files[0].sha256+="`n"} 'reject digest newline'
    Rejected {param($m) $m.files+=@($m.files[0])} 'reject duplicate member'
    Rejected {param($m) $m.files[0].path='../escape'} 'reject traversal'
    Rejected {param($m) $m.files[0].path='installer/CON.txt'} 'reject DOS device alias'
    Rejected {param($m) $m.files[0].path='installer/Install.ps1.'} 'reject trailing dot alias'
    Rejected {param($m) $m.files[0].path='package-manifest.json'} 'reject recursive manifest'
    Rejected {param($m) $m.files[0].PSObject.Properties.Remove('bytes')} 'reject missing member metadata'
    Rejected {param($m) $m.PSObject.Properties.Remove('platform')} 'reject missing platform'
    $script:baseline.releaseChannel='preview'
    $script:baseline.files=@($script:baseline.files | Where-Object path -notlike 'driver/*Source*')
    Valid
    Write-Output ('Package validation: ' + $script:checks + ' passed; synthetic files only, no Windows changes.')
} finally {
    $resolved = [IO.Path]::GetFullPath($testRoot).TrimEnd('\')
    if ($resolved -ne [IO.Path]::GetFullPath((Join-Path $tempParent $testName)).TrimEnd('\') -or -not $resolved.StartsWith($tempParent+'\',[StringComparison]::OrdinalIgnoreCase)) { throw 'Invalid test cleanup path.' }
    if (Test-Path -LiteralPath $resolved) {
        if ((Get-Item -LiteralPath $resolved).Attributes -band [IO.FileAttributes]::ReparsePoint) { throw 'Refusing redirected test cleanup.' }
        Remove-Item -LiteralPath $resolved -Recurse -Force
    }
}
