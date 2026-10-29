# Builds and crashes only an isolated test child; never opens a device or driver.
[CmdletBinding()]
param()
$ErrorActionPreference = 'Stop'
if ([Environment]::OSVersion.Platform -ne [PlatformID]::Win32NT) {
    throw 'This no_std fail-fast probe requires Windows.'
}
$repo = Split-Path -Parent (Split-Path -Parent $PSScriptRoot)
$kit = & (Join-Path $repo 'scripts/check-wdk.ps1') -AsObject
$target = Join-Path $PSScriptRoot 'target/panic-probe'
& (Join-Path $repo 'scripts/cargo.ps1') -CargoArgs @(
    'build','--manifest-path',(Join-Path $PSScriptRoot 'Cargo.toml'),
    '--release','--no-default-features','--locked','--offline','--target-dir',$target
)
$library = Join-Path $target 'release/libscx_source_policy.rlib'
$probe = Join-Path $target 'panic_probe.exe'
$rustcArgs = @(
    '--edition=2021',(Join-Path $PSScriptRoot 'tests/fixtures/panic_probe.rs'),
    '--crate-name','source_policy_panic_probe','--crate-type','bin',
    '-C','panic=abort','-C','opt-level=2','-C',"linker=$($kit.Link)",
    '-C','target-feature=+crt-static','-l','static=libcmt',
    '-l','static=libvcruntime','-l','static=libucrt',
    '-C','link-arg=/ENTRY:mainCRTStartup','-C','link-arg=/SUBSYSTEM:CONSOLE',
    '--extern',"scx_source_policy=$library",
    '-o',$probe
)
foreach ($path in @(
    (Join-Path $kit.MsvcRoot 'lib/x64'),
    (Join-Path $kit.SdkRoot "Lib/$($kit.SdkVersion)/ucrt/x64"),
    (Join-Path $kit.SdkRoot "Lib/$($kit.SdkVersion)/um/x64")
)) { $rustcArgs += @('-L',"native=$path") }
& rustc @rustcArgs
if ($LASTEXITCODE -ne 0) { throw 'Policy panic probe compilation/linking failed.' }
$child = New-Object Diagnostics.Process
$child.StartInfo.FileName = $probe
$child.StartInfo.UseShellExecute = $false
$child.StartInfo.CreateNoWindow = $true
$started = $false
try {
    $started = $child.Start()
    if (-not $started) { throw 'Policy panic probe could not start.' }
    if (-not $child.WaitForExit(10000)) {
        $child.Kill()
        [void]$child.WaitForExit(5000)
        throw 'Policy panic handler hung; the child was terminated after 10 seconds.'
    }
    $exitCode = [BitConverter]::ToUInt32([BitConverter]::GetBytes($child.ExitCode), 0)
    if ($exitCode -ne [Convert]::ToUInt32('C0000602', 16)) {
        throw ('Expected STATUS_FAIL_FAST_EXCEPTION, got 0x{0:X8}.' -f $exitCode)
    }
    Write-Output 'PASS: actual no_std policy panic terminated with STATUS_FAIL_FAST_EXCEPTION (0xC0000602).'
} finally {
    if ($started -and -not $child.HasExited) {
        $child.Kill()
        [void]$child.WaitForExit(5000)
    }
    $child.Dispose()
}
