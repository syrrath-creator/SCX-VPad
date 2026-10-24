[CmdletBinding()]
param([switch]$RequireWindowsPowerShell51)
$ErrorActionPreference = 'Stop'
Set-StrictMode -Version Latest
if ($RequireWindowsPowerShell51 -and ($PSVersionTable.PSEdition -ne 'Desktop' -or $PSVersionTable.PSVersion.Major -ne 5 -or $PSVersionTable.PSVersion.Minor -ne 1)) {
    throw 'This regression check must execute in Windows PowerShell 5.1, not merely parse there.'
}
$repoRoot = Split-Path -Parent $PSScriptRoot
$temporaryRoot = [IO.Path]::GetFullPath([IO.Path]::GetTempPath()).TrimEnd('\')
$testName = 'SCX-VPad-installer-test-' + [Guid]::NewGuid().ToString('N')
$testRoot = Join-Path $temporaryRoot $testName
$installRoot = Join-Path $testRoot 'owned'
$infRoot = Join-Path $testRoot 'fake-inf-store'

# Extract only the pure ownership resolver. Never run the installer/uninstaller
# entry point, elevate, touch HKLM, invoke pnputil, or change Windows trust.
$tokens = $null
$parseErrors = $null
$ast = [Management.Automation.Language.Parser]::ParseFile((Join-Path $repoRoot 'installer/Uninstall.ps1'), [ref]$tokens, [ref]$parseErrors)
if ($parseErrors.Count) { throw ($parseErrors | Out-String) }
$resolver = @($ast.FindAll({ param($node) $node -is [Management.Automation.Language.FunctionDefinitionAst] -and $node.Name -eq 'Get-OwnedDriverPackages' }, $true))
if ($resolver.Count -ne 1) { throw 'Expected one ownership resolver function.' }
. ([ScriptBlock]::Create($resolver[0].Extent.Text))

function Resolve-TestJournal($Entries) {
    $record = [pscustomobject]@{ DriverStageJournal = (ConvertTo-Json -InputObject $Entries -Depth 5 -Compress) }
    @(Get-OwnedDriverPackages $record $installRoot $infRoot)
}
function Assert-Rejected([ScriptBlock]$Operation, [string]$Description) {
    $rejected = $false
    try { & $Operation | Out-Null } catch { $rejected = $true }
    if (-not $rejected) { throw $Description }
}
try {
    New-Item -ItemType Directory -Path (Join-Path $installRoot 'driver'), $infRoot -Force | Out-Null
    $hidInf = Join-Path $installRoot 'driver/scxvpad.inf'
    $xusbInf = Join-Path $installRoot 'driver/scxxusb.inf'
    Set-Content -LiteralPath $hidInf -Value 'Synthetic SCX HID package'
    Set-Content -LiteralPath $xusbInf -Value 'Synthetic SCX XUSB package'
    $hidHash = (Get-FileHash -LiteralPath $hidInf -Algorithm SHA256).Hash
    $xusbHash = (Get-FileHash -LiteralPath $xusbInf -Algorithm SHA256).Hash
    Copy-Item -LiteralPath $hidInf -Destination (Join-Path $infRoot 'oem1.inf')
    Copy-Item -LiteralPath $hidInf -Destination (Join-Path $infRoot 'oem2.inf')
    Copy-Item -LiteralPath $xusbInf -Destination (Join-Path $infRoot 'oem3.inf')
    Set-Content -LiteralPath (Join-Path $infRoot 'oem4.inf') -Value 'Unrelated package'
    $journal = @(
        [pscustomobject]@{ inf='scxvpad.inf'; property='PublishedInf'; sha256=$hidHash; baseline=@('oem1.inf'); attempted=$true; published=@() },
        [pscustomobject]@{ inf='scxxusb.inf'; property='PublishedXusbInf'; sha256=$xusbHash; baseline=@('oem1.inf'); attempted=$true; published=@() }
    )

    $recovered = @(Resolve-TestJournal $journal)
    if ((@($recovered.name | Sort-Object) -join ',') -ne 'oem2.inf,oem3.inf') { throw 'Partial-import recovery or baseline isolation failed.' }
    Write-Output 'PASS 1/7: both interrupted imports recovered; pre-existing/unrelated packages excluded'

    $journal[0].published = @('oem1.inf')
    Assert-Rejected { Resolve-TestJournal $journal } 'Accepted ownership of a baseline package.'
    Write-Output 'PASS 2/7: baseline ownership claim rejected'

    $journal[0].published = @('oem4.inf')
    Assert-Rejected { Resolve-TestJournal $journal } 'Accepted a recorded package with a different hash.'
    Write-Output 'PASS 3/7: recorded unrelated package rejected by hash'

    $journal[0].published = @()
    $journal[0].attempted = $false
    if ((@(Resolve-TestJournal $journal).name -join ',') -ne 'oem3.inf') { throw 'An unattempted import acquired ownership.' }
    Write-Output 'PASS 4/7: unattempted import owns no package'

    $journal[0].inf = '../other.inf'
    Assert-Rejected { Resolve-TestJournal $journal } 'Accepted a non-allowlisted/traversing journal INF.'
    Write-Output 'PASS 5/7: non-allowlisted INF path rejected'

    $legacy = [pscustomobject]@{ PublishedInf='oem2.inf' }
    if ((@(Get-OwnedDriverPackages $legacy $installRoot $infRoot).name -join ',') -ne 'oem2.inf') { throw 'Legacy record claimed an unrecorded package.' }
    Write-Output 'PASS 6/7: legacy record owns only its explicitly named matching INF'

    $beforeStaging = [pscustomobject]@{ InstallDirectory=$installRoot }
    if (@(Get-OwnedDriverPackages $beforeStaging $installRoot $infRoot).Count -ne 0) { throw 'An interrupted pre-staging install claimed a driver.' }
    Write-Output 'PASS 7/7: pre-staging failure owns no driver'

    $sourceInf = Join-Path $installRoot 'driver/scxsource.inf'
    Set-Content -LiteralPath $sourceInf -Value 'Synthetic SCX Source Guard package'
    $sourceHash = (Get-FileHash -LiteralPath $sourceInf -Algorithm SHA256).Hash
    Copy-Item -LiteralPath $sourceInf -Destination (Join-Path $infRoot 'oem5.inf')
    Copy-Item -LiteralPath $sourceInf -Destination (Join-Path $infRoot 'oem6.inf')
    $journal[0].inf = 'scxvpad.inf'; $journal[0].attempted = $true
    $sourceStage = [pscustomobject]@{ inf='scxsource.inf'; property='PublishedSourceInf'; sha256=$sourceHash; baseline=@('oem1.inf','oem5.inf'); attempted=$true; published=@() }
    $sourceJournal = @($journal) + @($sourceStage)
    if ((@(Resolve-TestJournal $sourceJournal).name | Sort-Object) -join ',' -ne 'oem2.inf,oem3.inf,oem6.inf') { throw 'Source Guard ownership recovery failed.' }
    Write-Output 'PASS: Source Guard partial import recovered; pre-existing filter excluded'
    $sourceStage.published = @('oem5.inf')
    Assert-Rejected { Resolve-TestJournal $sourceJournal } 'Claimed an existing physical source filter.'
    $sourceStage.published = @()
    Assert-Rejected { Resolve-TestJournal @($journal[0], $sourceStage) } 'Accepted a journal missing the XUSB package.'
    Assert-Rejected { Resolve-TestJournal @($journal[0], $journal[0], $sourceStage) } 'Accepted duplicate driver journal entries.'
    Write-Output 'PASS: Source Guard baseline, incomplete and duplicate journals rejected'
} finally {
    # Resolve the exact unique temporary directory before the only recursive
    # removal. Its children are test-created files; no Windows paths are used.
    $resolved = [IO.Path]::GetFullPath($testRoot).TrimEnd('\')
    $expected = [IO.Path]::GetFullPath((Join-Path $temporaryRoot $testName)).TrimEnd('\')
    if ($resolved -ne $expected -or -not $resolved.StartsWith($temporaryRoot + '\', [StringComparison]::OrdinalIgnoreCase)) { throw 'Test cleanup escaped its temporary directory.' }
    if (Test-Path -LiteralPath $resolved) {
        if ((Get-Item -LiteralPath $resolved).Attributes -band [IO.FileAttributes]::ReparsePoint) { throw 'Refusing redirected test cleanup.' }
        Remove-Item -LiteralPath $resolved -Recurse -Force
    }
}
Write-Output ('Installer recovery: 11 passed on PowerShell ' + $PSVersionTable.PSVersion)
