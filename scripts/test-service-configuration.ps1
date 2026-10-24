[CmdletBinding()]
param([switch]$RequireWindowsPowerShell51)
$ErrorActionPreference='Stop'
Set-StrictMode -Version Latest
if ($RequireWindowsPowerShell51 -and ($PSVersionTable.PSEdition -ne 'Desktop' -or $PSVersionTable.PSVersion -lt [version]'5.1' -or $PSVersionTable.PSVersion.Major -ne 5)) { throw 'Run this check in Windows PowerShell 5.1.' }
. (Join-Path $PSScriptRoot '../installer/Service.ps1')
$expected='"C:\Program Files\SCX VPad\bin\scx-vpad-service.exe"'
if ((Get-ScxServiceCommandLine 'C:\Program Files\SCX VPad\bin\scx-vpad-service.exe') -cne $expected) { throw 'Space-containing service path was not preserved.' }
foreach ($bad in @('relative.exe','\rooted.exe','C:relative.exe','C:\app.exe --mode','"C:\app.exe"',"C:\app`n.exe",'\\server\app.exe','C:\app:stream.exe')) {
    $rejected=$false
    try { Get-ScxServiceCommandLine $bad | Out-Null } catch { $rejected=$true }
    if (-not $rejected) { throw ('Accepted ambiguous service executable path: '+$bad) }
}
# A genuine read-only SCM query exercises native layout/Unicode marshalling.
# No service is created, changed, stopped, started or deleted by this check.
$existing=Get-Service -Name ScxVPad -ErrorAction SilentlyContinue
if ($existing) {
    $scm=[Scx.ServiceConfiguration]::ReadImage('ScxVPad')
    $registry=(Get-ItemProperty 'HKLM:\SYSTEM\CurrentControlSet\Services\ScxVPad').ImagePath
    if ($scm -cne $registry) { throw 'SCM and persisted service command lines differ.' }
    if (-not $scm.StartsWith('"')) { throw 'Installed SCM service path is unquoted.' }
    Write-Output 'PASS: genuine SCM query agrees with persisted quoted configuration.'
}
Write-Output 'PASS: absolute service path quoting and eight ambiguous/injected-path rejections.'
