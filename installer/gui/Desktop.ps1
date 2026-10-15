[CmdletBinding()]
param(
    [Parameter(Mandatory=$true)][ValidateSet('install','replace','remove')][string]$Action,
    [Parameter(Mandatory=$true)][ValidatePattern('^S-1-5-21-(\d+-){3}\d+$')][string]$OwnerSid
)
$ErrorActionPreference = 'Stop'
Set-StrictMode -Version Latest
[Console]::OutputEncoding = New-Object Text.UTF8Encoding($false)
$root = $PSScriptRoot
$key = 'HKLM:\SOFTWARE\SCX\VirtualPad'
try {
    if ($Action -ne 'install') {
        $record = Get-ItemProperty -LiteralPath $key
        # A reinstall retains the account already authorized by the driver.
        if ($record.PSObject.Properties.Name -contains 'InstallOwnerSid') {
            if ($record.InstallOwnerSid -notmatch '^S-1-5-21-(\d+-){3}\d+$') { throw 'Invalid recorded Windows account.' }
            $OwnerSid = [string]$record.InstallOwnerSid
        }
        & (Join-Path $root 'installer\Uninstall.ps1')
        if (Test-Path -LiteralPath $key) { throw 'Removal did not complete. No new installation was attempted.' }
        $deadline = (Get-Date).AddSeconds(15)
        while (Get-Service -Name ScxVPad -ErrorAction SilentlyContinue) {
            if ((Get-Date) -ge $deadline) { throw 'Windows is still removing the service. Restart Windows, then run Setup again.' }
            Start-Sleep -Milliseconds 250
        }
    }
    if ($Action -ne 'remove') {
        & (Join-Path $root 'installer\Install.ps1') -AuthorizedUserSid $OwnerSid
        $record = Get-ItemProperty -LiteralPath $key
        if ($record.InstallComplete -ne 1 -or $record.InstallOwnerSid -ne $OwnerSid) { throw 'Installation verification failed.' }
        $service = Get-Service -Name ScxVPad
        if ($service.Status -ne 'Running') { throw 'The SCX VPad service did not start.' }
        Write-Output 'Verified: SCX VPad installation complete; service running; original Windows account authorized.'
    }
    exit 0
} catch {
    Write-Output ('ERROR: ' + $_.Exception.Message)
    Write-Output $_.ScriptStackTrace
    exit 1
}
