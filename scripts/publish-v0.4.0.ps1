# One-release publisher. Never overwrites an existing tag, release, or asset.
[CmdletBinding()]
param(
    [switch]$ValidateOnly,
    [string]$RepositoryRoot = (Split-Path -Parent $PSScriptRoot)
)
Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'
$ProgressPreference = 'SilentlyContinue'
$repo = 'syrrath-creator/SCX-VPad'
$tag = 'v0.4.0'
$allowed = @('SCX-VPad-0.4.0-Setup-x64.exe','SCX-VPad-0.4.0-windows-x64.zip','RELEASE-QUALIFICATION.txt','SHA256SUMS.txt')
$root = [IO.Path]::GetFullPath($RepositoryRoot).TrimEnd('\','/')
$artifactRoot = Join-Path $root 'release-artifacts'

function Assert-PlainPath([string]$Path, [string]$Boundary) {
    $full = [IO.Path]::GetFullPath($Path)
    $base = [IO.Path]::GetFullPath($Boundary).TrimEnd('\','/')
    if (-not $full.StartsWith($base + [IO.Path]::DirectorySeparatorChar, [StringComparison]::OrdinalIgnoreCase)) { throw 'Path outside expected directory.' }
    $cursor = Get-Item -LiteralPath $full -Force
    while ($null -ne $cursor) {
        if ($cursor.Attributes -band [IO.FileAttributes]::ReparsePoint) { throw 'Reparse paths are not allowed.' }
        if ($cursor.FullName.TrimEnd('\','/') -eq $base) { return }
        $cursor = if ($cursor -is [IO.DirectoryInfo]) { $cursor.Parent } else { $cursor.Directory }
    }
    throw 'Path did not resolve within expected directory.'
}

function Assert-Properties($Object, [string[]]$Names) {
    if ($null -eq $Object -or $Object -isnot [pscustomobject]) { throw 'Expected a JSON object.' }
    $actual = @($Object.PSObject.Properties.Name)
    if ($actual.Count -ne $Names.Count -or @($actual | Where-Object { $_ -cnotin $Names }).Count) { throw 'Unexpected manifest properties.' }
}

function Get-HexHash([byte[]]$Bytes) {
    [Convert]::ToHexString([Security.Cryptography.SHA256]::HashData($Bytes)).ToLowerInvariant()
}

# All local data is checked before an API token is read or any network request occurs.
Assert-PlainPath $artifactRoot $root
$manifestPath = Join-Path $artifactRoot 'publication.json'
Assert-PlainPath $manifestPath $artifactRoot
if ((Get-Item -LiteralPath $manifestPath).Length -gt 131072) { throw 'Publication manifest is too large.' }
$publication = Get-Content -LiteralPath $manifestPath -Raw | ConvertFrom-Json
Assert-Properties $publication @('tagName','targetCommit','title','body','artifacts')
if ($publication.tagName -cne $tag -or $publication.targetCommit -isnot [string] -or $publication.targetCommit -cnotmatch '\A[0-9a-f]{40}\z') { throw 'Wrong release tag or target commit.' }
if ($publication.title -isnot [string] -or [string]::IsNullOrWhiteSpace($publication.title) -or $publication.title.Length -gt 200 -or $publication.body -isnot [string] -or [string]::IsNullOrWhiteSpace($publication.body) -or $publication.body.Length -gt 60000) { throw 'Invalid release text.' }
if (@($publication.artifacts).Count -ne $allowed.Count) { throw 'Exactly four release artifacts are required.' }
$local = @{}
foreach ($entry in $publication.artifacts) {
    Assert-Properties $entry @('name','sha256','size')
    if ($entry.name -isnot [string] -or $entry.name -cnotin $allowed -or $local.ContainsKey($entry.name)) { throw 'Unexpected or duplicate release artifact.' }
    if ($entry.sha256 -isnot [string] -or $entry.sha256 -cnotmatch '\A[0-9a-f]{64}\z' -or ($entry.size -isnot [int] -and $entry.size -isnot [long]) -or $entry.size -le 0 -or $entry.size -gt 67108864) { throw 'Invalid artifact size or SHA256.' }
    $path = Join-Path $artifactRoot $entry.name
    Assert-PlainPath $path $artifactRoot
    if ((Get-Item -LiteralPath $path).PSIsContainer) { throw 'Artifact must be a regular file.' }
    $bytes = [IO.File]::ReadAllBytes($path)
    if ($bytes.LongLength -ne $entry.size -or (Get-HexHash $bytes) -cne $entry.sha256) { throw ('Artifact verification failed: ' + $entry.name) }
    # Retain the exact verified bytes; uploads do not reread writable files.
    $local[$entry.name] = @{ name=$entry.name; size=[long]$entry.size; sha256=$entry.sha256; bytes=$bytes }
}
$actualFiles = @(Get-ChildItem -LiteralPath $artifactRoot -Force)
if ($actualFiles.Count -ne 5 -or @($actualFiles | Where-Object { $_.PSIsContainer -or $_.Name -cnotin ($allowed + 'publication.json') }).Count) { throw 'Unexpected release-artifacts contents.' }
$sumLines = @([Text.Encoding]::UTF8.GetString($local['SHA256SUMS.txt'].bytes).TrimStart([char]0xfeff) -split '\r?\n' | Where-Object { $_.Length -gt 0 })
if ($sumLines.Count -ne 3) { throw 'SHA256SUMS must list exactly the other three assets.' }
$sumSeen = @{}
foreach ($line in $sumLines) {
    if ($line -cnotmatch '\A([0-9a-f]{64})  ([A-Za-z0-9._-]+)\z') { throw 'Malformed checksum line.' }
    $hash = $Matches[1]; $name = $Matches[2]
    if ($name -cnotin $allowed -or $name -ceq 'SHA256SUMS.txt' -or -not $local.ContainsKey($name) -or $sumSeen.ContainsKey($name) -or $local[$name].sha256 -cne $hash) { throw 'Checksum inventory mismatch.' }
    $sumSeen[$name] = $true
}

# Extract only validated archive members to a fresh directory; execute the
# existing validator from the trusted checkout, never a script from the ZIP.
$tempRoot = Join-Path ([IO.Path]::GetTempPath()) ('scx-release04-' + [guid]::NewGuid().ToString('N'))
[void][IO.Directory]::CreateDirectory($tempRoot)
$zipStream = [IO.MemoryStream]::new($local['SCX-VPad-0.4.0-windows-x64.zip'].bytes, $false)
$archive = [IO.Compression.ZipArchive]::new($zipStream, [IO.Compression.ZipArchiveMode]::Read)
try {
    $zipNames = @{}; $zipAllNames = @{}; [long]$total = 0
    if ($archive.Entries.Count -lt 12 -or $archive.Entries.Count -gt 256) { throw 'Invalid ZIP member count.' }
    foreach ($member in $archive.Entries) {
        $isDirectory = $member.FullName.EndsWith('/')
        $name = if ($isDirectory) { $member.FullName.Substring(0, $member.FullName.Length - 1) } else { $member.FullName }
        if ($name -cnotmatch '\A[A-Za-z0-9][A-Za-z0-9_./ -]*\z' -or $name.Contains('//')) { throw 'Invalid ZIP member path.' }
        foreach ($part in ($name -split '/')) {
            if (-not $part -or $part -in @('.','..') -or $part.EndsWith('.') -or $part.EndsWith(' ') -or $part -match '\A(CON|PRN|AUX|NUL|COM[1-9]|LPT[1-9])(\.|\z)') { throw 'Ambiguous ZIP member path.' }
        }
        if ($zipAllNames.ContainsKey($name) -or $member.Length -lt 0 -or $member.Length -gt 33554432 -or (($member.ExternalAttributes -shr 16) -band 0xf000) -eq 0xa000) { throw 'Duplicate, symlink, or oversized ZIP member.' }
        $zipAllNames[$name] = $true; $total += $member.Length
        if ($total -gt 67108864) { throw 'ZIP expanded size exceeds the package limit.' }
        $destination = [IO.Path]::GetFullPath((Join-Path $tempRoot $name))
        if (-not $destination.StartsWith($tempRoot + [IO.Path]::DirectorySeparatorChar, [StringComparison]::OrdinalIgnoreCase)) { throw 'ZIP traversal rejected.' }
        if ($isDirectory) {
            if ($member.Length -ne 0) { throw 'ZIP directory has a payload.' }
            continue # File extraction creates parents; directory entries need no writes.
        }
        $zipNames[$name] = $true
        [void][IO.Directory]::CreateDirectory((Split-Path -Parent $destination))
        $inputStream = $member.Open(); $outputStream = [IO.File]::Open($destination, [IO.FileMode]::CreateNew)
        try {
            $buffer = [byte[]]::new(65536); [long]$written = 0
            while (($read = $inputStream.Read($buffer, 0, $buffer.Length)) -gt 0) {
                $written += $read
                if ($written -gt $member.Length) { throw 'ZIP member exceeds declared size.' }
                $outputStream.Write($buffer, 0, $read)
            }
        } finally { $outputStream.Dispose(); $inputStream.Dispose() }
        if ((Get-Item -LiteralPath $destination).Length -ne $member.Length) { throw 'ZIP member size mismatch.' }
    }
    $validator = Join-Path $root 'installer/Package.ps1'
    Assert-PlainPath $validator $root
    . $validator
    $package = Get-ScxVerifiedPackage $tempRoot
    # Release 0.4.0 retains SDK/package schema 0.1.0 and native version .39.
    if ($package.version -cne '0.1.0' -or $package.driverVersion -cne '0.1.0.39' -or $package.releaseChannel -cne 'stable') { throw 'ZIP is not the reviewed stable package schema 0.1.0 / native .39 package.' }
    if ($zipNames.Count -ne @($package.files).Count + 1) { throw 'ZIP contains unmanifested members.' }
} finally {
    $archive.Dispose(); $zipStream.Dispose()
    $cleanupPath = [IO.Path]::GetFullPath($tempRoot)
    $tempBoundary = [IO.Path]::GetFullPath([IO.Path]::GetTempPath()).TrimEnd('\','/') + [IO.Path]::DirectorySeparatorChar
    if (-not $cleanupPath.StartsWith($tempBoundary, [StringComparison]::OrdinalIgnoreCase) -or [IO.Path]::GetFileName($cleanupPath) -cnotmatch '\Ascx-release04-[0-9a-f]{32}\z') { throw 'Refusing cleanup outside the created scratch directory.' }
    Remove-Item -LiteralPath $cleanupPath -Recurse -Force
}
Write-Output ('Validated four reviewed assets and stable package for ' + $publication.targetCommit + '.')
if ($ValidateOnly) { return }
if ($env:GITHUB_ACTIONS -cne 'true' -or $env:GITHUB_REPOSITORY -cne $repo -or $env:GITHUB_EVENT_NAME -cne 'push' -or $env:GITHUB_REF -cne 'refs/heads/release/v0.4.0' -or $env:GITHUB_SHA -cnotmatch '\A[0-9a-f]{40}\z') { throw 'Publishing is restricted to the designated repository and branch push.' }
if ([string]::IsNullOrWhiteSpace($env:GITHUB_TOKEN)) { throw 'The workflow token is missing.' }
$handler = [Net.Http.HttpClientHandler]::new(); $handler.AllowAutoRedirect = $false
$http = [Net.Http.HttpClient]::new($handler); $http.Timeout = [TimeSpan]::FromMinutes(2)

function Send-GitHub([string]$Method, [string]$Url, [object]$Json = $null, [byte[]]$Bytes = $null, [switch]$Allow404, [string]$Accept = 'application/vnd.github+json') {
    $uri = [uri]$Url
    if ($uri.Scheme -cne 'https' -or $uri.Host -cnotin @('api.github.com','uploads.github.com') -or $uri.UserInfo -or -not $uri.IsDefaultPort) { throw 'Refusing authenticated request to an unexpected host.' }
    $request = [Net.Http.HttpRequestMessage]::new([Net.Http.HttpMethod]::new($Method), $uri)
    $request.Headers.Authorization = [Net.Http.Headers.AuthenticationHeaderValue]::new('Bearer', $env:GITHUB_TOKEN)
    [void]$request.Headers.TryAddWithoutValidation('Accept', $Accept)
    [void]$request.Headers.TryAddWithoutValidation('User-Agent', 'SCX-VPad-reviewed-release-0.4.0')
    [void]$request.Headers.TryAddWithoutValidation('X-GitHub-Api-Version', '2022-11-28')
    if ($null -ne $Bytes) { $request.Content = [Net.Http.ByteArrayContent]::new($Bytes); $request.Content.Headers.ContentType = [Net.Http.Headers.MediaTypeHeaderValue]::new('application/octet-stream') }
    elseif ($null -ne $Json) { $request.Content = [Net.Http.StringContent]::new(($Json | ConvertTo-Json -Depth 10 -Compress), [Text.Encoding]::UTF8, 'application/json') }
    try {
        $response = $http.SendAsync($request).GetAwaiter().GetResult()
        try {
            $status = [int]$response.StatusCode
            if ($Allow404 -and $status -eq 404) { return $null }
            if (-not $response.IsSuccessStatusCode) { throw ('GitHub ' + $Method + ' failed with HTTP ' + $status + '; no existing data is overwritten.') }
            $text = $response.Content.ReadAsStringAsync().GetAwaiter().GetResult()
            if ($text.Length -gt 2097152) { throw 'Unexpectedly large GitHub JSON response.' }
            if ($text) { return ($text | ConvertFrom-Json) }
        } finally { $response.Dispose() }
    } finally { $request.Dispose() }
}

function Assert-TagTarget {
    $ref = Send-GitHub GET "https://api.github.com/repos/$repo/git/ref/tags/$tag"
    if ($ref.object.type -cne 'commit' -or $ref.object.sha -cne $publication.targetCommit) { throw 'Release tag does not point to the reviewed target commit.' }
}

function Get-DownloadedAssetHash([long]$AssetId, [long]$ExpectedSize) {
    $url = "https://api.github.com/repos/$repo/releases/assets/$AssetId"
    for ($redirect = 0; $redirect -le 5; $redirect++) {
        $uri = [uri]$url
        if ($uri.Scheme -cne 'https' -or $uri.UserInfo -or -not $uri.IsDefaultPort -or $uri.Host -cnotin @('api.github.com','github.com','release-assets.githubusercontent.com','objects.githubusercontent.com','github-releases.githubusercontent.com')) { throw 'Unexpected asset download host.' }
        $request = [Net.Http.HttpRequestMessage]::new([Net.Http.HttpMethod]::Get, $uri)
        [void]$request.Headers.TryAddWithoutValidation('User-Agent', 'SCX-VPad-reviewed-release-0.4.0')
        [void]$request.Headers.TryAddWithoutValidation('Accept', 'application/octet-stream')
        if ($redirect -eq 0) { $request.Headers.Authorization = [Net.Http.Headers.AuthenticationHeaderValue]::new('Bearer', $env:GITHUB_TOKEN) }
        try {
            $response = $http.SendAsync($request, [Net.Http.HttpCompletionOption]::ResponseHeadersRead).GetAwaiter().GetResult()
            try {
                if ([int]$response.StatusCode -in @(301,302,303,307,308)) {
                    if ($null -eq $response.Headers.Location) { throw 'Download redirect is missing a location.' }
                    $url = [uri]::new($uri, $response.Headers.Location).AbsoluteUri
                    continue # No Authorization header is forwarded to any redirect.
                }
                if (-not $response.IsSuccessStatusCode) { throw ('Asset download failed with HTTP ' + [int]$response.StatusCode) }
                if ($null -ne $response.Content.Headers.ContentLength -and $response.Content.Headers.ContentLength -ne $ExpectedSize) { throw 'Asset download length mismatch.' }
                $stream = $response.Content.ReadAsStreamAsync().GetAwaiter().GetResult()
                $memory = [IO.MemoryStream]::new(); $buffer = [byte[]]::new(65536)
                try {
                    while (($read = $stream.Read($buffer, 0, $buffer.Length)) -gt 0) {
                        if ($memory.Length + $read -gt $ExpectedSize) { throw 'Asset download exceeds expected size.' }
                        $memory.Write($buffer, 0, $read)
                    }
                    if ($memory.Length -ne $ExpectedSize) { throw 'Asset download truncated.' }
                    return (Get-HexHash $memory.ToArray())
                } finally { $memory.Dispose(); $stream.Dispose() }
            } finally { $response.Dispose() }
        } finally { $request.Dispose() }
    }
    throw 'Too many asset download redirects.'
}

function Assert-RemoteAssets([long]$ReleaseId) {
    $assets = @(Send-GitHub GET "https://api.github.com/repos/$repo/releases/$ReleaseId/assets?per_page=100")
    if ($assets.Count -ne 4) { throw 'Release asset count mismatch.' }
    $seen = @{}
    foreach ($asset in $assets) {
        if ($asset.name -cnotin $allowed -or $seen.ContainsKey($asset.name) -or $asset.state -cne 'uploaded' -or $asset.size -ne $local[$asset.name].size) { throw 'Unexpected remote release asset.' }
        $seen[$asset.name] = $true
        $digest = if ($asset.PSObject.Properties.Name -contains 'digest') { $asset.digest } else { $null }
        if ([string]::IsNullOrEmpty($digest)) { $hash = Get-DownloadedAssetHash $asset.id $asset.size }
        elseif ($digest -cmatch '\Asha256:([0-9a-f]{64})\z') { $hash = $Matches[1] }
        else { throw 'Unexpected GitHub asset digest format.' }
        if ($hash -cne $local[$asset.name].sha256) { throw ('Remote asset hash mismatch: ' + $asset.name) }
    }
}

try {
    $main = Send-GitHub GET "https://api.github.com/repos/$repo/git/ref/heads/main"
    if ($main.object.type -cne 'commit' -or $main.object.sha -cne $publication.targetCommit) { throw 'Reviewed targetCommit must be the current main commit.' }
    if ($null -ne (Send-GitHub GET "https://api.github.com/repos/$repo/releases/tags/$tag" -Allow404) -or $null -ne (Send-GitHub GET "https://api.github.com/repos/$repo/git/ref/tags/$tag" -Allow404)) { throw 'Tag or release already exists; refusing overwrite.' }
    $deadline = [DateTime]::UtcNow.AddMinutes(15)
    do {
        $runs = Send-GitHub GET "https://api.github.com/repos/$repo/actions/workflows/verify.yml/runs?head_sha=$($publication.targetCommit)&event=push&per_page=100"
        $passed = @($runs.workflow_runs | Where-Object { $_.head_sha -ceq $publication.targetCommit -and $_.event -ceq 'push' -and $_.head_branch -ceq 'main' -and $_.status -ceq 'completed' -and $_.conclusion -ceq 'success' })
        if ($passed.Count -gt 0) { Write-Output ('Verified successful main CI run ' + $passed[0].id + '.'); break }
        if ([DateTime]::UtcNow -ge $deadline) { throw 'No successful verify.yml main push run for the exact target SHA within 15 minutes.' }
        Write-Output 'Waiting for verification of the exact source commit.'
        Start-Sleep -Seconds 15
    } while ($true)
    # Recheck after the CI wait, before the first mutation.
    $main = Send-GitHub GET "https://api.github.com/repos/$repo/git/ref/heads/main"
    if ($main.object.sha -cne $publication.targetCommit) { throw 'Main changed during verification.' }
    if ($null -ne (Send-GitHub GET "https://api.github.com/repos/$repo/releases/tags/$tag" -Allow404) -or $null -ne (Send-GitHub GET "https://api.github.com/repos/$repo/git/ref/tags/$tag" -Allow404)) { throw 'Tag or release appeared during verification; refusing overwrite.' }
    $draft = Send-GitHub POST "https://api.github.com/repos/$repo/releases" @{tag_name=$tag;target_commitish=$publication.targetCommit;name=$publication.title;body=$publication.body;draft=$true;prerelease=$false;make_latest='false';generate_release_notes=$false}
    if (-not $draft.draft -or $draft.tag_name -cne $tag -or $draft.id -le 0) { throw 'Unexpected release creation response.' }
    Write-Output ('Created draft release ' + $draft.id + '; failures leave it for inspection, without overwriting or deleting data.')
    foreach ($name in $allowed) {
        $asset = $local[$name]
        $uploaded = Send-GitHub POST "https://uploads.github.com/repos/$repo/releases/$($draft.id)/assets?name=$([uri]::EscapeDataString($name))" -Bytes $asset.bytes
        if ($uploaded.name -cne $name -or $uploaded.size -ne $asset.size -or $uploaded.state -cne 'uploaded') { throw 'Upload response does not match the reviewed artifact.' }
        Write-Output ('Uploaded ' + $name + '.')
    }
    Assert-RemoteAssets $draft.id
    # Atomic create: a concurrent existing tag returns 422, never force-update it.
    [void](Send-GitHub POST "https://api.github.com/repos/$repo/git/refs" @{ref="refs/tags/$tag";sha=$publication.targetCommit})
    Assert-TagTarget
    $published = Send-GitHub PATCH "https://api.github.com/repos/$repo/releases/$($draft.id)" @{draft=$false;prerelease=$false;make_latest='true'}
    $final = Send-GitHub GET "https://api.github.com/repos/$repo/releases/$($draft.id)"
    if ($final.draft -or $final.prerelease -or $final.tag_name -cne $tag -or $final.name -cne $publication.title -or $final.body -cne $publication.body -or $null -eq $final.published_at) { throw 'Published release state did not verify.' }
    Assert-TagTarget
    Assert-RemoteAssets $draft.id
    $latest = Send-GitHub GET "https://api.github.com/repos/$repo/releases/latest"
    if ($latest.id -ne $draft.id) { throw 'Release did not become latest stable.' }
    Write-Output ('Published and verified ' + $final.html_url)
} finally { $http.Dispose(); $handler.Dispose() }
