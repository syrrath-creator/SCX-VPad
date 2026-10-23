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

function Get-ValidatedReplacementIdentity($Record, [Microsoft.Win32.RegistryValueKind]$ValueKind) {
    foreach ($name in @('InstallDirectory','InstallOwnerSid','DeviceIdentitySeed')) {
        if ($Record.PSObject.Properties.Name -notcontains $name) { throw ('Missing installed identity field: ' + $name) }
    }
    $expectedDirectory = Join-Path ([Environment]::GetFolderPath([Environment+SpecialFolder]::ProgramFiles)) 'SCX VPad'
    if ($Record.InstallDirectory -isnot [string] -or -not [string]::Equals($Record.InstallDirectory, $expectedDirectory, [StringComparison]::OrdinalIgnoreCase)) { throw 'Unexpected installed SCX directory.' }
    if ($Record.InstallOwnerSid -isnot [string] -or $Record.InstallOwnerSid -notmatch '^S-1-5-21-(\d+-){3}\d+\z') { throw 'Invalid recorded Windows account.' }
    if ($ValueKind -ne [Microsoft.Win32.RegistryValueKind]::QWord -or ($Record.DeviceIdentitySeed -isnot [Int64] -and $Record.DeviceIdentitySeed -isnot [UInt64])) { throw 'Installed identity seed must be a REG_QWORD.' }
    # Registry QWORDs may be exposed as signed Int64. Preserve all 64 bits,
    # including the high bit; a numeric UInt64-to-Int64 cast would overflow.
    $seed = if ($Record.DeviceIdentitySeed -is [UInt64]) {
        [BitConverter]::ToInt64([BitConverter]::GetBytes([UInt64]$Record.DeviceIdentitySeed), 0)
    } else { [Int64]$Record.DeviceIdentitySeed }
    if ($seed -eq 0) { throw 'Installed identity seed must be nonzero.' }
    [pscustomobject]@{OwnerSid=[string]$Record.InstallOwnerSid; Seed=$seed}
}

function Restore-ReplacementIdentity($Identity) {
    $identityKey = 'HKLM:\SOFTWARE\SCX\VirtualPad'
    $current = Get-ItemProperty -LiteralPath $identityKey
    $validated = Get-ValidatedReplacementIdentity $current ((Get-Item -LiteralPath $identityKey).GetValueKind('DeviceIdentitySeed'))
    if ($validated.OwnerSid -ne $Identity.OwnerSid) { throw 'Replacement authorized a different Windows account.' }
    Stop-Service -Name ScxVPad -ErrorAction Stop
    (Get-Service -Name ScxVPad).WaitForStatus('Stopped', [TimeSpan]::FromSeconds(25))
    New-ItemProperty -LiteralPath $identityKey -Name DeviceIdentitySeed -PropertyType QWord -Value $Identity.Seed -Force | Out-Null
    $current = Get-ValidatedReplacementIdentity (Get-ItemProperty -LiteralPath $identityKey) ((Get-Item -LiteralPath $identityKey).GetValueKind('DeviceIdentitySeed'))
    if ($current.OwnerSid -ne $Identity.OwnerSid -or $current.Seed -ne $Identity.Seed) { throw 'Identity seed restoration failed; service left stopped.' }
    Start-Service -Name ScxVPad -ErrorAction Stop
    (Get-Service -Name ScxVPad).WaitForStatus('Running', [TimeSpan]::FromSeconds(25))
    $current = Get-ValidatedReplacementIdentity (Get-ItemProperty -LiteralPath $identityKey) ((Get-Item -LiteralPath $identityKey).GetValueKind('DeviceIdentitySeed'))
    if ($current.OwnerSid -ne $Identity.OwnerSid -or $current.Seed -ne $Identity.Seed) { throw 'Replacement identity changed after service restart.' }
}

$replacementIdentity = $null
try {
    if ($Action -ne 'install') {
        $record = Get-ItemProperty -LiteralPath $key
        # A reinstall retains the account already authorized by the driver.
        if ($record.PSObject.Properties.Name -contains 'InstallOwnerSid') {
            if ($record.InstallOwnerSid -notmatch '^S-1-5-21-(\d+-){3}\d+$') { throw 'Invalid recorded Windows account.' }
            $OwnerSid = [string]$record.InstallOwnerSid
        }
        if ($Action -eq 'replace') {
            # Validate and save before uninstall removes the installation key.
            $replacementIdentity = Get-ValidatedReplacementIdentity $record ((Get-Item -LiteralPath $key).GetValueKind('DeviceIdentitySeed'))
            $OwnerSid = $replacementIdentity.OwnerSid
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
        if ($null -ne $replacementIdentity) { Restore-ReplacementIdentity $replacementIdentity }
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
