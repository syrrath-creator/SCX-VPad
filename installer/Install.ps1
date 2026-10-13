[CmdletBinding()]
param([string]$AuthorizedUserSid)
$ErrorActionPreference = 'Stop'
Set-StrictMode -Version Latest
if (-not [Environment]::Is64BitProcess) { throw 'Run this installer in 64-bit PowerShell; 32-bit registry and filesystem redirection are not supported.' }
$osArchitecture = (Get-ItemProperty 'HKLM:\SYSTEM\CurrentControlSet\Control\Session Manager\Environment').PROCESSOR_ARCHITECTURE
if ($osArchitecture -ne 'AMD64') { throw 'This package requires an x64 Windows installation; ARM64 is not supported.' }
$windowsDirectory = [Environment]::GetFolderPath([Environment+SpecialFolder]::Windows)
$programFilesDirectory = [Environment]::GetFolderPath([Environment+SpecialFolder]::ProgramFiles)
$commonDataDirectory = [Environment]::GetFolderPath([Environment+SpecialFolder]::CommonApplicationData)
$serviceName = 'ScxVPad'
$productKey = 'HKLM:\SOFTWARE\SCX\VirtualPad'
$identity = [Security.Principal.WindowsIdentity]::GetCurrent()
if (-not $AuthorizedUserSid) { $AuthorizedUserSid = $identity.User.Value }
if ($AuthorizedUserSid -notmatch '^S-1-5-21-(\d+-){3}\d+$') {
    throw 'Provide the SID of the local interactive user who will use SCX VPad.'
}
$isAdmin = ([Security.Principal.WindowsPrincipal]$identity).IsInRole([Security.Principal.WindowsBuiltInRole]::Administrator)
if (-not $isAdmin) {
    # The invoking user is carried through an over-the-shoulder administrator login.
    # Process-local RemoteSigned permits this locally built script. It does not
    # change machine/user policy or bypass downloaded-script signature checks.
    $arguments = '-NoProfile -ExecutionPolicy RemoteSigned -File "' + $PSCommandPath + '" -AuthorizedUserSid "' + $AuthorizedUserSid + '"'
    $child = Start-Process -FilePath (Join-Path $windowsDirectory 'System32\WindowsPowerShell\v1.0\powershell.exe') -ArgumentList $arguments -Verb RunAs -WindowStyle Hidden -PassThru -Wait
    if ($child.ExitCode -ne 0) { throw "Installation failed ($($child.ExitCode)); see Program Files\SCX VPad\logs if setup reached staging." }
    return
}
$packageRoot = Split-Path -Parent $PSScriptRoot
$manifestPath = Join-Path $packageRoot 'package-manifest.json'
if (-not (Test-Path -LiteralPath $manifestPath)) { throw 'Run the installer from a complete release package.' }
$manifest = Get-Content -LiteralPath $manifestPath -Raw | ConvertFrom-Json
if ($manifest.product -ne 'SCX-VPad' -or $manifest.version -notmatch '^\d+\.\d+\.\d+$') { throw 'Invalid package identity.' }
function Assert-LocalPackagePath([string]$FullPath, [string]$RootPath) {
    $cursor = Get-Item -LiteralPath $FullPath
    while ($null -ne $cursor) {
        if ($cursor.Attributes -band [IO.FileAttributes]::ReparsePoint) { throw 'Redirected package paths are not supported.' }
        if ($cursor.FullName.TrimEnd('\') -eq $RootPath.TrimEnd('\')) { return }
        $cursor = if ($cursor -is [IO.DirectoryInfo]) { $cursor.Parent } else { $cursor.Directory }
    }
    throw 'Package path did not resolve inside its root.'
}
$buildNumber = [int](Get-ItemProperty 'HKLM:\SOFTWARE\Microsoft\Windows NT\CurrentVersion').CurrentBuildNumber
if ($buildNumber -lt 22000 -or -not [Environment]::Is64BitOperatingSystem) { throw 'This build requires Windows 11 x64.' }
if (Test-Path -LiteralPath $productKey) { throw 'SCX VPad is already installed. Uninstall the existing development build first.' }
if (Get-Service -Name $serviceName -ErrorAction SilentlyContinue) { throw 'The SCX VPad service name is already in use.' }
$packagePrefix = [IO.Path]::GetFullPath($packageRoot).TrimEnd('\') + '\'
$seenPaths = @{}
foreach ($entry in $manifest.files) {
    if ($entry.path -notmatch '^[A-Za-z0-9][A-Za-z0-9_./\\ -]*$' -or [IO.Path]::IsPathRooted($entry.path)) { throw 'Invalid relative manifest path.' }
    foreach ($segment in ($entry.path -split '[\\/]')) {
        if (-not $segment -or $segment -eq '.' -or $segment -eq '..' -or $segment.EndsWith('.') -or $segment.EndsWith(' ')) { throw 'Ambiguous manifest path component.' }
    }
    if ($entry.sha256 -notmatch '^[A-Fa-f0-9]{64}$') { throw 'Invalid manifest digest.' }
    $path = [IO.Path]::GetFullPath((Join-Path $packageRoot $entry.path))
    if (-not $path.StartsWith($packagePrefix, [StringComparison]::OrdinalIgnoreCase)) { throw 'Manifest path escaped package.' }
    if ($seenPaths.ContainsKey($path)) { throw 'Duplicate manifest path.' }
    $seenPaths[$path] = $true
    if (-not (Test-Path -LiteralPath $path -PathType Leaf)) { throw "Missing package file: $($entry.path)" }
    Assert-LocalPackagePath $path $packageRoot
    if ((Get-FileHash -LiteralPath $path -Algorithm SHA256).Hash -ne $entry.sha256) { throw "Package hash mismatch: $($entry.path)" }
}
$destination = [IO.Path]::GetFullPath((Join-Path $programFilesDirectory 'SCX VPad'))
if (Test-Path -LiteralPath $destination) { throw 'The installation directory already exists; refusing to overwrite it.' }
$createdDestination = $false
$createdRecord = $false
$recordReady = $false
$transcriptStarted = $false
$infDirectory = Join-Path $windowsDirectory 'INF'
try {
    New-Item -ItemType Directory -Path $destination | Out-Null
    $createdDestination = $true
    $directoryAcl = New-Object Security.AccessControl.DirectorySecurity
    $directoryAcl.SetSecurityDescriptorSddlForm('O:BAG:BAD:PAI(A;OICI;FA;;;SY)(A;OICI;FA;;;BA)(A;OICI;GRGX;;;BU)')
    Set-Acl -LiteralPath $destination -AclObject $directoryAcl
    # A recoverable ownership record exists before staging any executable or
    # changing Windows trust. Earlier failures remove only this new empty root.
    if (Test-Path -LiteralPath $productKey) { throw 'Installation record appeared concurrently; refusing to overwrite it.' }
    New-Item -Path $productKey -Force | Out-Null
    $createdRecord = $true
    New-ItemProperty -Path $productKey -Name InstallDirectory -Value $destination -PropertyType String -Force | Out-Null
    $registryAcl = New-Object Security.AccessControl.RegistrySecurity
    $registryAcl.SetSecurityDescriptorSddlForm('O:BAG:BAD:PAI(A;CI;KA;;;SY)(A;CI;KA;;;BA)(A;CI;KR;;;BU)')
    Set-Acl -Path $productKey -AclObject $registryAcl
    $recordReady = $true
    New-ItemProperty -Path $productKey -Name InstallOwnerSid -Value $AuthorizedUserSid -PropertyType String -Force | Out-Null
    New-ItemProperty -Path $productKey -Name Version -Value $manifest.version -PropertyType String -Force | Out-Null
    # ProgramData is user-creatable. Logs instead inherit the protected root's
    # administrator/SYSTEM-only write permissions; never follow a supplied path.
    $logRoot = Join-Path $destination 'logs'
    New-Item -ItemType Directory -Path $logRoot | Out-Null
    $logPath = Join-Path $logRoot ('setup-' + [Guid]::NewGuid().ToString('N') + '.txt')
    Start-Transcript -LiteralPath $logPath -NoClobber | Out-Null
    $transcriptStarted = $true
    # Copy only validated manifest members, never the caller's whole directory.
    foreach ($entry in $manifest.files) {
        $target = [IO.Path]::GetFullPath((Join-Path $destination $entry.path))
        if (-not $target.StartsWith($destination.TrimEnd('\') + '\', [StringComparison]::OrdinalIgnoreCase)) { throw 'Manifest target escaped the protected destination.' }
        New-Item -ItemType Directory -Path (Split-Path -Parent $target) -Force | Out-Null
        Copy-Item -LiteralPath (Join-Path $packageRoot $entry.path) -Destination $target
        if ((Get-FileHash -LiteralPath $target -Algorithm SHA256).Hash -ne $entry.sha256) { throw 'Copied package changed during staging.' }
    }
    # Serialize the already validated snapshot, not a second mutable source read.
    $manifest | ConvertTo-Json -Depth 8 | Set-Content -LiteralPath (Join-Path $destination 'package-manifest.json') -Encoding utf8
    $certificate = New-SelfSignedCertificate -Type CodeSigningCert -Subject ('CN=SCX VPad local ' + [Guid]::NewGuid().ToString('N')) -FriendlyName 'SCX VPad local driver signing' -CertStoreLocation 'Cert:\LocalMachine\My' -KeyAlgorithm RSA -KeyLength 3072 -HashAlgorithm SHA256 -KeyExportPolicy NonExportable -NotAfter (Get-Date).AddYears(3)
    New-ItemProperty -Path $productKey -Name CertificateThumbprint -Value $certificate.Thumbprint -PropertyType String -Force | Out-Null
    $rsa = [Security.Cryptography.X509Certificates.RSACertificateExtensions]::GetRSAPrivateKey($certificate)
    try {
        if ($rsa -isnot [Security.Cryptography.RSACng] -or -not $rsa.Key.IsMachineKey) { throw 'Expected a machine-local CNG signing key.' }
        $keyDirectory = [IO.Path]::GetFullPath((Join-Path $commonDataDirectory 'Microsoft\Crypto\Keys')).TrimEnd('\') + '\'
        $keyPath = [IO.Path]::GetFullPath((Join-Path $keyDirectory $rsa.Key.UniqueName))
        if (-not $keyPath.StartsWith($keyDirectory, [StringComparison]::OrdinalIgnoreCase)) { throw 'Signing key path escaped the machine key directory.' }
        $keyAcl = New-Object Security.AccessControl.FileSecurity
        $keyAcl.SetSecurityDescriptorSddlForm('O:BAG:BAD:P(A;;FA;;;SY)(A;;FA;;;BA)')
        Set-Acl -LiteralPath $keyPath -AclObject $keyAcl
    } finally { if ($rsa) { $rsa.Dispose() } }
    $publicCertificate = New-Object Security.Cryptography.X509Certificates.X509Certificate2 -ArgumentList @(,$certificate.Export([Security.Cryptography.X509Certificates.X509ContentType]::Cert))
    foreach ($storeName in @('Root','TrustedPublisher')) {
        $store = New-Object Security.Cryptography.X509Certificates.X509Store($storeName, 'LocalMachine')
        $store.Open([Security.Cryptography.X509Certificates.OpenFlags]::ReadWrite)
        try { $store.Add($publicCertificate) } finally { $store.Close() }
    }
    # The driver catalogue covers the built INF and DLL. Sign only the catalogue
    # here so no covered file changes after Inf2Cat generated its member hashes.
    foreach ($relative in @('driver\scxvpad.cat','bin\scx-vpad-service.exe','bin\scx-vpadctl.exe','bin\xinput_probe.exe')) {
        $signedPath = Join-Path $destination $relative
        $signature = Set-AuthenticodeSignature -LiteralPath $signedPath -Certificate $certificate -HashAlgorithm SHA256
        if ($signature.Status -ne 'Valid') { throw "Local signing failed for $relative : $($signature.StatusMessage)" }
    }
    # Journal BOTH INFs before either import. A failed/aborted pnputil can still
    # stage a package, so deletion must recover by exact hash minus the baseline.
    $baseline = @(Get-ChildItem -LiteralPath $infDirectory -Filter 'oem*.inf' -File | ForEach-Object Name)
    $journal = @(
        foreach ($package in @(@('scxvpad.inf','PublishedInf'), @('scxxusb.inf','PublishedXusbInf'))) {
            $infHash = (Get-FileHash -LiteralPath (Join-Path $destination ('driver\' + $package[0])) -Algorithm SHA256).Hash
            $existing = @(Get-ChildItem -LiteralPath $infDirectory -Filter 'oem*.inf' -File | Where-Object { (Get-FileHash -LiteralPath $_.FullName -Algorithm SHA256).Hash -eq $infHash })
            if ($existing.Count) { throw 'An identical SCX INF already exists. Refusing to claim ownership of a pre-existing driver package.' }
            [pscustomobject]@{ inf=$package[0]; property=$package[1]; sha256=$infHash; baseline=$baseline; attempted=$false; published=@() }
        }
    )
    New-ItemProperty -Path $productKey -Name DriverStageJournal -Value (ConvertTo-Json -InputObject $journal -Depth 5 -Compress) -PropertyType String -Force | Out-Null
    foreach ($stage in $journal) {
        $inf = Join-Path $destination ('driver\' + $stage.inf)
        $stage.attempted = $true
        New-ItemProperty -Path $productKey -Name DriverStageJournal -Value (ConvertTo-Json -InputObject $journal -Depth 5 -Compress) -PropertyType String -Force | Out-Null
        try {
            & (Join-Path $windowsDirectory 'System32\pnputil.exe') /add-driver $inf /install
            $driverExit = $LASTEXITCODE
        } finally {
            $stage.published = @(Get-ChildItem -LiteralPath $infDirectory -Filter 'oem*.inf' -File | Where-Object { $_.Name -notin $stage.baseline -and (Get-FileHash -LiteralPath $_.FullName -Algorithm SHA256).Hash -eq $stage.sha256 } | ForEach-Object Name)
            New-ItemProperty -Path $productKey -Name DriverStageJournal -Value (ConvertTo-Json -InputObject $journal -Depth 5 -Compress) -PropertyType String -Force | Out-Null
            if ($stage.published.Count -eq 1) {
                New-ItemProperty -Path $productKey -Name $stage.property -Value $stage.published[0] -PropertyType String -Force | Out-Null
            }
        }
        if ($driverExit -ne 0 -and $driverExit -ne 3010) { throw "Driver package installation failed: $driverExit" }
        if ($stage.published.Count -ne 1) { throw 'Could not uniquely record the installed driver package; its staging journal is retained for safe recovery.' }
    }
    $serviceExe = Join-Path $destination 'bin\scx-vpad-service.exe'
    . (Join-Path $destination 'installer\GameInput.ps1')
    Install-ScxGameInputMappings
    & (Join-Path $windowsDirectory 'System32\sc.exe') create $serviceName binPath= ('"' + $serviceExe + '"') start= demand obj= LocalSystem DisplayName= 'SCX VPad Device Manager'
    if ($LASTEXITCODE -ne 0) { throw "Creating service failed: $LASTEXITCODE" }
    # Windows PowerShell 5's native argument marshalling can strip quotes passed
    # through sc.exe. Persist the literal quoted path and check it before start.
    $quotedServicePath = '"' + $serviceExe + '"'
    $serviceKey = 'HKLM:\SYSTEM\CurrentControlSet\Services\ScxVPad'
    Set-ItemProperty -LiteralPath $serviceKey -Name ImagePath -Value $quotedServicePath
    if ((Get-ItemProperty -LiteralPath $serviceKey).ImagePath -ne $quotedServicePath) { throw 'Service executable path was not quoted correctly.' }
    $serviceSddl = 'D:(A;;CCDCLCSWRPWPDTLOCRSDRCWDWO;;;SY)(A;;CCDCLCSWRPWPDTLOCRSDRCWDWO;;;BA)(A;;CCLCLORPRC;;;' + $AuthorizedUserSid + ')'
    & (Join-Path $windowsDirectory 'System32\sc.exe') sdset $serviceName $serviceSddl
    if ($LASTEXITCODE -ne 0) { throw 'Failed to restrict service control permissions.' }
    Start-Service -Name $serviceName
    New-ItemProperty -Path $productKey -Name InstallComplete -Value 1 -PropertyType DWord -Force | Out-Null
    Write-Output 'SCX VPad installed. Ordinary clients can now use the service without elevation.'
} catch {
    if (-not $recordReady) {
        # No files, certificate, driver or service have been staged yet. Only
        # remove roots created by this invocation, with absolute path checks.
        if ($createdRecord) { Remove-Item -LiteralPath $productKey -Recurse -Force }
        if ($createdDestination) {
            $expectedRoot = [IO.Path]::GetFullPath((Join-Path $programFilesDirectory 'SCX VPad'))
            if ($destination -ne $expectedRoot -or ((Get-Item -LiteralPath $destination).Attributes -band [IO.FileAttributes]::ReparsePoint)) { throw 'Refusing cleanup of an unexpected installation root.' }
            Remove-Item -LiteralPath $destination -Recurse -Force
        }
    }
    Write-Error -Message ('SCX VPad setup failed. If an installation record exists, copy Program Files\SCX VPad\logs before running the packaged Uninstall.ps1; it recovers owned partial installs. ' + $_.Exception.Message) -ErrorAction Continue
    throw
} finally { if ($transcriptStarted) { Stop-Transcript | Out-Null } }

