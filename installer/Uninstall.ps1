[CmdletBinding()]
param()
$ErrorActionPreference = 'Stop'
Set-StrictMode -Version Latest
if (-not [Environment]::Is64BitProcess) { throw 'Run this uninstaller in 64-bit PowerShell.' }
$osArchitecture = (Get-ItemProperty 'HKLM:\SYSTEM\CurrentControlSet\Control\Session Manager\Environment').PROCESSOR_ARCHITECTURE
if ($osArchitecture -ne 'AMD64') { throw 'This package requires an x64 Windows installation.' }
$windowsDirectory = [Environment]::GetFolderPath([Environment+SpecialFolder]::Windows)
$programFilesDirectory = [Environment]::GetFolderPath([Environment+SpecialFolder]::ProgramFiles)
$identity = [Security.Principal.WindowsIdentity]::GetCurrent()
if (-not ([Security.Principal.WindowsPrincipal]$identity).IsInRole([Security.Principal.WindowsBuiltInRole]::Administrator)) {
    $child = Start-Process -FilePath (Join-Path $windowsDirectory 'System32\WindowsPowerShell\v1.0\powershell.exe') -ArgumentList ('-NoProfile -ExecutionPolicy RemoteSigned -File "' + $PSCommandPath + '"') -Verb RunAs -WindowStyle Hidden -PassThru -Wait
    if ($child.ExitCode -ne 0) { throw "Uninstallation failed: $($child.ExitCode)" }
    return
}
$key = 'HKLM:\SOFTWARE\SCX\VirtualPad'
if (-not (Test-Path -LiteralPath $key)) { throw 'No SCX VPad installation record exists; refusing speculative removal.' }
$record = Get-ItemProperty -LiteralPath $key
$expected = [IO.Path]::GetFullPath((Join-Path $programFilesDirectory 'SCX VPad')).TrimEnd('\')
$actual = [IO.Path]::GetFullPath($record.InstallDirectory).TrimEnd('\')
if ($actual -ne $expected) { throw 'Installation directory does not match the fixed SCX VPad location.' }
if ((Test-Path -LiteralPath $actual) -and ((Get-Item -LiteralPath $actual).Attributes -band [IO.FileAttributes]::ReparsePoint)) { throw 'Refusing to remove a redirected installation directory.' }
function Get-OwnedDriverPackages($InstallRecord, [string]$InstallRoot, [string]$InfRoot) {
    $known = @{ 'scxvpad.inf'='PublishedInf'; 'scxxusb.inf'='PublishedXusbInf' }
    $result = @{}
    if ($InstallRecord.PSObject.Properties.Name -contains 'DriverStageJournal') {
        # Windows PowerShell 5.1 emits a JSON array as one pipeline object.
        # Assign first, then normalize; @(... | ConvertFrom-Json) nests it.
        $decodedJournal = ConvertFrom-Json -InputObject $InstallRecord.DriverStageJournal
        $journal = @($decodedJournal)
        if ($journal.Count -ne 2) { throw 'Incomplete driver staging journal; refusing speculative removal.' }
        $seen = @{}
        foreach ($stage in $journal) {
            foreach ($field in @('inf','property','sha256','baseline','attempted','published')) {
                if ($stage.PSObject.Properties.Name -notcontains $field) { throw 'Missing driver journal field.' }
            }
            if (-not $known.ContainsKey([string]$stage.inf) -or $stage.property -ne $known[[string]$stage.inf] -or $seen.ContainsKey([string]$stage.inf)) { throw 'Unknown or duplicate INF in staging journal.' }
            $seen[[string]$stage.inf] = $true
            if ($stage.sha256 -notmatch '^[A-Fa-f0-9]{64}$' -or $stage.attempted -isnot [bool]) { throw 'Invalid driver staging metadata.' }
            foreach ($name in @($stage.baseline) + @($stage.published)) {
                if ($name -notmatch '^oem\d+\.inf$') { throw 'Invalid OEM name in staging journal.' }
            }
            if (-not $stage.attempted) {
                if (@($stage.published).Count) { throw 'An unattempted import cannot own a driver package.' }
                continue
            }
            $ownInf = Join-Path $InstallRoot ('driver\' + $stage.inf)
            if ((Test-Path -LiteralPath $ownInf) -and (Get-FileHash -LiteralPath $ownInf -Algorithm SHA256).Hash -ne $stage.sha256) { throw 'Protected staging INF changed.' }
            # A previous pnputil may have staged the INF before returning an
            # error or before setup could persist its published name. Recover
            # only exact expected hashes absent from the pre-install baseline.
            $found = @(Get-ChildItem -LiteralPath $InfRoot -Filter 'oem*.inf' -File | Where-Object { $_.Name -notin $stage.baseline -and (Get-FileHash -LiteralPath $_.FullName -Algorithm SHA256).Hash -eq $stage.sha256 } | ForEach-Object Name)
            foreach ($name in @($stage.published) + $found) {
                if ($name -in $stage.baseline) { throw 'Refusing to remove a pre-existing driver package.' }
                $installed = Join-Path $InfRoot $name
                if (Test-Path -LiteralPath $installed) {
                    if ((Get-FileHash -LiteralPath $installed -Algorithm SHA256).Hash -ne $stage.sha256) { throw 'Recorded driver INF changed; refusing to remove another package.' }
                    $result[$name] = [string]$stage.sha256
                }
            }
        }
    } else {
        # Backward-compatible removal of earlier development installations.
        # Without a journal, only an explicitly recorded name may be removed.
        foreach ($infName in $known.Keys) {
            $property = $known[$infName]
            if ($InstallRecord.PSObject.Properties.Name -notcontains $property) { continue }
            $name = $InstallRecord.$property
            if ($name -notmatch '^oem\d+\.inf$') { throw 'Invalid recorded driver package name.' }
            $installed = Join-Path $InfRoot $name
            if (Test-Path -LiteralPath $installed) {
                $expectedHash = (Get-FileHash -LiteralPath (Join-Path $InstallRoot ('driver\' + $infName)) -Algorithm SHA256).Hash
                if ((Get-FileHash -LiteralPath $installed -Algorithm SHA256).Hash -ne $expectedHash) { throw 'Driver INF changed; refusing to remove another package.' }
                $result[$name] = $expectedHash
            }
        }
    }
    foreach ($name in $result.Keys) { [pscustomobject]@{ name=$name; sha256=$result[$name] } }
}
$infDirectory = Join-Path $windowsDirectory 'INF'
# Resolve and validate owned packages before stopping/removing anything.
$ownedPackages = @(Get-OwnedDriverPackages $record $actual $infDirectory)
$service = Get-Service -Name ScxVPad -ErrorAction SilentlyContinue
if ($service) {
    . (Join-Path $PSScriptRoot 'Service.ps1')
    $expectedBinary = '"' + (Join-Path $actual 'bin\scx-vpad-service.exe') + '"'
    if ([Scx.ServiceConfiguration]::ReadImage('ScxVPad') -cne $expectedBinary) { throw 'Service binary path changed; refusing to remove a different service.' }
}
if ($record.PSObject.Properties.Name -contains 'GameInputJournal') {
    . (Join-Path $PSScriptRoot 'GameInput.ps1')
    Restore-ScxGameInputMappings
}
if ($service) {
    Stop-Service -Name ScxVPad
    & (Join-Path $windowsDirectory 'System32\sc.exe') delete ScxVPad
    if ($LASTEXITCODE -ne 0) { throw 'Removing SCX VPad service failed.' }
}
foreach ($package in $ownedPackages) {
    $installedInf = Join-Path $infDirectory $package.name
    if (Test-Path -LiteralPath $installedInf) {
        if ((Get-FileHash -LiteralPath $installedInf -Algorithm SHA256).Hash -ne $package.sha256) { throw 'Driver INF changed during removal.' }
        & (Join-Path $windowsDirectory 'System32\pnputil.exe') /delete-driver $package.name /uninstall
        if ($LASTEXITCODE -ne 0 -and $LASTEXITCODE -ne 3010) { throw 'Removing SCX VPad driver package failed.' }
    }
}
if ($record.PSObject.Properties.Name -contains 'CertificateThumbprint') {
    $thumb = $record.CertificateThumbprint
    if ($thumb -notmatch '^[A-Fa-f0-9]{40}$') { throw 'Invalid recorded certificate thumbprint.' }
    foreach ($store in @('Root','TrustedPublisher','My')) {
        $certPath = "Cert:\LocalMachine\$store\$thumb"
        if (Test-Path -LiteralPath $certPath) {
            $cert = Get-Item -LiteralPath $certPath
            if ($cert.Subject -notlike 'CN=SCX VPad local *') { throw 'Certificate identity does not match SCX VPad.' }
            if ($store -eq 'My') { Remove-Item -LiteralPath $certPath -DeleteKey }
            else { Remove-Item -LiteralPath $certPath }
        }
    }
}
# The absolute target has been checked against the single fixed product folder.
if (Test-Path -LiteralPath $actual) { Remove-Item -LiteralPath $actual -Recurse -Force }
Remove-Item -LiteralPath $key -Recurse
Write-Output 'SCX VPad removed. Other controller drivers and certificates were not changed.'

