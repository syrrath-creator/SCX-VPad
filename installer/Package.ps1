# Pure package validation; no elevation, installation, registry or trust changes.
function Get-ScxVerifiedPackage([string]$PackageRoot) {
    $root = [IO.Path]::GetFullPath($PackageRoot).TrimEnd('\')
    $prefix = $root + '\'
    $manifestPath = Join-Path $root 'package-manifest.json'
    if (-not (Test-Path -LiteralPath $manifestPath -PathType Leaf) -or (Get-Item -LiteralPath $manifestPath).Length -gt 1048576) { throw 'Missing or oversized package manifest.' }
    Assert-ScxPlainPackagePath $manifestPath $root
    $manifest = Get-Content -LiteralPath $manifestPath -Raw | ConvertFrom-Json
    foreach ($field in @('product','version','driverVersion','platform','minimumBuild','files')) {
        if ($manifest.PSObject.Properties.Name -notcontains $field) { throw ('Missing package identity field: ' + $field) }
    }
    if ($manifest.product -cne 'SCX-VPad' -or $manifest.version -notmatch '\A\d+\.\d+\.\d+\z' -or $manifest.driverVersion -notmatch '\A\d+\.\d+\.\d+\.\d+\z' -or $manifest.platform -cne 'windows-x64' -or $manifest.minimumBuild -ne 22000) { throw 'Invalid package identity or platform.' }
    if ($manifest.PSObject.Properties.Name -contains 'releaseChannel' -and $manifest.releaseChannel -cnotin @('preview','stable')) { throw 'Invalid release channel.' }
    if (@($manifest.files).Count -lt 12 -or @($manifest.files).Count -gt 256) { throw 'Invalid package member count.' }
    $seen = @{}
    [Int64]$total = 0
    foreach ($entry in $manifest.files) {
        foreach ($field in @('path','bytes','sha256')) { if ($entry.PSObject.Properties.Name -notcontains $field) { throw 'Incomplete package member.' } }
        if ($entry.path -isnot [string] -or $entry.path -notmatch '\A[A-Za-z0-9][A-Za-z0-9_./\\ -]*\z' -or [IO.Path]::IsPathRooted($entry.path)) { throw 'Invalid relative manifest path.' }
        foreach ($segment in ($entry.path -split '[\\/]')) {
            if (-not $segment -or $segment -in @('.','..') -or $segment.EndsWith('.') -or $segment.EndsWith(' ') -or $segment -match '\A(CON|PRN|AUX|NUL|COM[1-9]|LPT[1-9])(\.|\z)') { throw 'Ambiguous manifest path component.' }
        }
        if ($entry.sha256 -notmatch '\A[A-Fa-f0-9]{64}\z' -or ($entry.bytes -isnot [Int32] -and $entry.bytes -isnot [Int64]) -or $entry.bytes -lt 0 -or $entry.bytes -gt 33554432) { throw 'Invalid package member size or digest.' }
        $relative = $entry.path.Replace('\','/')
        if ($relative -ieq 'package-manifest.json' -or $seen.ContainsKey($relative)) { throw 'Duplicate or recursive manifest path.' }
        $path = [IO.Path]::GetFullPath((Join-Path $root $entry.path))
        if (-not $path.StartsWith($prefix, [StringComparison]::OrdinalIgnoreCase)) { throw 'Manifest path escaped package.' }
        if (-not (Test-Path -LiteralPath $path -PathType Leaf)) { throw ('Missing package file: ' + $relative) }
        Assert-ScxPlainPackagePath $path $root
        $total += $entry.bytes
        if ($total -gt 67108864 -or (Get-Item -LiteralPath $path).Length -ne $entry.bytes -or (Get-FileHash -LiteralPath $path -Algorithm SHA256).Hash -ne $entry.sha256) { throw ('Package member verification failed: ' + $relative) }
        $seen[$relative] = $true
    }
    foreach ($required in @('installer/Install.ps1','installer/Uninstall.ps1','installer/GameInput.ps1','installer/Service.ps1','installer/Package.ps1','driver/ScxVPad.dll','driver/scxvpad.inf','driver/scxxusb.inf','driver/scxvpad.cat','bin/scx-vpad-service.exe','bin/scx-vpadctl.exe','bin/xinput_probe.exe','LICENSE','THIRD-PARTY-NOTICES.txt')) {
        if (-not $seen.ContainsKey($required)) { throw ('Incomplete package: ' + $required) }
    }
    $sourceCount = @(@('driver/ScxSource.dll','driver/scxsource.inf','driver/scxsource.cat') | Where-Object { $seen.ContainsKey($_) }).Count
    if ($sourceCount -notin @(0,3)) { throw 'Incomplete SCX Source Guard package.' }
    if ($manifest.PSObject.Properties.Name -contains 'releaseChannel' -and $manifest.releaseChannel -ceq 'stable' -and $sourceCount -ne 3) { throw 'Stable packages must include Source Guard.' }
    return $manifest
}

function Assert-ScxPlainPackagePath([string]$FullPath, [string]$RootPath) {
    $cursor = Get-Item -LiteralPath $FullPath
    while ($null -ne $cursor) {
        if ($cursor.Attributes -band [IO.FileAttributes]::ReparsePoint) { throw 'Redirected package paths are not supported.' }
        if ($cursor.FullName.TrimEnd('\') -eq $RootPath.TrimEnd('\')) { return }
        $cursor = if ($cursor -is [IO.DirectoryInfo]) { $cursor.Parent } else { $cursor.Directory }
    }
    throw 'Package path did not resolve inside its root.'
}
