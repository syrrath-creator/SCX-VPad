[CmdletBinding()]
param(
    [string]$WdkRoot = $env:SCX_WDK_ROOT,
    [string]$RustLibrary,
    [string]$OutputDirectory,
    [switch]$SkipCatalog
)
$ErrorActionPreference = 'Stop'
$repoRoot = Split-Path -Parent $PSScriptRoot
$kit = & (Join-Path $PSScriptRoot 'check-wdk.ps1') -WdkRoot $WdkRoot -AsObject
if (-not $RustLibrary) { $RustLibrary = Join-Path $repoRoot 'target\release\scx_vpad_core.lib' }
if (-not $OutputDirectory) { $OutputDirectory = Join-Path $repoRoot 'target\driver\x64' }
$RustLibrary = [IO.Path]::GetFullPath($RustLibrary)
$OutputDirectory = [IO.Path]::GetFullPath($OutputDirectory)
if (-not (Test-Path -LiteralPath $RustLibrary)) {
    throw "Build the Rust static library first: cargo build --release -p scx-vpad-core. Expected $RustLibrary"
}
New-Item -ItemType Directory -Path $OutputDirectory -Force | Out-Null
$includePaths = @(
    (Join-Path $repoRoot 'include'),
    (Join-Path $kit.WdkRoot 'Include\wdf\umdf\2.25'),
    (Join-Path $kit.MsvcRoot 'include'),
    (Join-Path $kit.SdkRoot "Include\$($kit.SdkVersion)\ucrt"),
    (Join-Path $kit.SdkRoot "Include\$($kit.SdkVersion)\um"),
    (Join-Path $kit.SdkRoot "Include\$($kit.SdkVersion)\shared"),
    (Join-Path $kit.WdkRoot "Include\$($kit.SdkVersion)\shared"),
    (Join-Path $kit.WdkRoot "Include\$($kit.SdkVersion)\um"),
    (Join-Path $kit.WdkRoot "Include\$($kit.SdkVersion)\km")
)
$libPaths = @(
    (Join-Path $kit.MsvcRoot 'lib\x64'),
    (Join-Path $kit.SdkRoot "Lib\$($kit.SdkVersion)\ucrt\x64"),
    (Join-Path $kit.SdkRoot "Lib\$($kit.SdkVersion)\um\x64")
)
$objectPath = Join-Path $OutputDirectory 'scxvpad.obj'
$xusbObjectPath = Join-Path $OutputDirectory 'scxxusb.obj'
$resourcePath = Join-Path $OutputDirectory 'scxvpad.res'
$dllPath = Join-Path $OutputDirectory 'ScxVPad.dll'
$compileArgs = @('/nologo','/c','/TC','/std:c17','/O2','/MT','/GS','/guard:cf','/analyze','/analyze:external-',
    '/W4','/WX','/DWIN32_LEAN_AND_MEAN','/DUNICODE','/D_UNICODE',
    '/DUMDF_VERSION_MAJOR=2','/DUMDF_VERSION_MINOR=25','/DUMDF_USING_NTSTATUS',
    "/Fo$objectPath",(Join-Path $repoRoot 'driver\scxvpad.c')) +
    ($includePaths | ForEach-Object { "/I$_" })
& $kit.Cl @compileArgs
if ($LASTEXITCODE -ne 0) { throw "C binding compilation failed ($LASTEXITCODE)." }
$xusbCompileArgs = $compileArgs | ForEach-Object {
    if ($_ -eq "/Fo$objectPath") { "/Fo$xusbObjectPath" }
    elseif ($_ -eq (Join-Path $repoRoot 'driver\scxvpad.c')) { Join-Path $repoRoot 'driver\scxxusb.c' }
    else { $_ }
}
& $kit.Cl @xusbCompileArgs
if ($LASTEXITCODE -ne 0) { throw "XUSB binding compilation failed ($LASTEXITCODE)." }
$resourceArgs = @('/nologo',"/fo$resourcePath") +
    ($includePaths | ForEach-Object { "/I$_" }) + (Join-Path $repoRoot 'driver\ScxVPad.rc')
& $kit.Rc @resourceArgs
if ($LASTEXITCODE -ne 0) { throw "Version resource compilation failed ($LASTEXITCODE)." }
$linkArgs = @('/nologo','/DLL','/MACHINE:X64','/SUBSYSTEM:WINDOWS,10.00',
    '/DYNAMICBASE','/NXCOMPAT','/HIGHENTROPYVA','/guard:cf','/CETCOMPAT',
    '/OPT:REF','/OPT:ICF','/INCREMENTAL:NO','/Brepro',
    "/DEF:$(Join-Path $repoRoot 'driver\ScxVPad.def')", "/OUT:$dllPath",
    $objectPath,$xusbObjectPath,$resourcePath,$RustLibrary,$kit.WdfLibrary,
    'kernel32.lib','ntdll.lib','advapi32.lib','userenv.lib','ws2_32.lib','bcrypt.lib') +
    ($libPaths | ForEach-Object { "/LIBPATH:$_" })
& $kit.Link @linkArgs
if ($LASTEXITCODE -ne 0) { throw "UMDF2 DLL linking failed ($LASTEXITCODE)." }
$abiObject = Join-Path $OutputDirectory 'abi_check.obj'
$abiExe = Join-Path $OutputDirectory 'abi_check.exe'
$abiCompile = @('/nologo','/c','/TC','/std:c17','/O2','/MT','/W4','/WX',
    "/Fo$abiObject",(Join-Path $repoRoot 'driver\abi_check.c')) + ($includePaths | ForEach-Object { "/I$_" })
& $kit.Cl @abiCompile
if ($LASTEXITCODE -ne 0) { throw "Native ABI check compilation failed ($LASTEXITCODE)." }
$abiLink = @('/nologo','/MACHINE:X64','/SUBSYSTEM:CONSOLE','/DYNAMICBASE','/NXCOMPAT',
    "/OUT:$abiExe",$abiObject,$RustLibrary,'kernel32.lib','ntdll.lib') + ($libPaths | ForEach-Object { "/LIBPATH:$_" })
& $kit.Link @abiLink
if ($LASTEXITCODE -ne 0) { throw "Native ABI check linking failed ($LASTEXITCODE)." }
& $abiExe
if ($LASTEXITCODE -ne 0) { throw "Native ABI check failed ($LASTEXITCODE)." }
$probeObject = Join-Path $OutputDirectory 'xinput_probe.obj'
$probeExe = Join-Path $OutputDirectory 'xinput_probe.exe'
$probeCompile = @('/nologo','/c','/TC','/std:c17','/O2','/MT','/W4','/WX','/GS','/guard:cf',
    "/Fo$probeObject",(Join-Path $repoRoot 'driver\xinput_probe.c')) + ($includePaths | ForEach-Object { "/I$_" })
& $kit.Cl @probeCompile
if ($LASTEXITCODE -ne 0) { throw "Independent XInput probe compilation failed ($LASTEXITCODE)." }
$probeLink = @('/nologo','/MACHINE:X64','/SUBSYSTEM:CONSOLE','/DYNAMICBASE','/NXCOMPAT','/guard:cf','/CETCOMPAT',
    "/OUT:$probeExe",$probeObject,'kernel32.lib') + ($libPaths | ForEach-Object { "/LIBPATH:$_" })
& $kit.Link @probeLink
if ($LASTEXITCODE -ne 0) { throw "Independent XInput probe linking failed ($LASTEXITCODE)." }
Copy-Item -LiteralPath (Join-Path $repoRoot 'driver\scxvpad.inf') -Destination $OutputDirectory -Force
Copy-Item -LiteralPath (Join-Path $repoRoot 'driver\scxxusb.inf') -Destination $OutputDirectory -Force
foreach ($infName in @('scxvpad.inf','scxxusb.inf')) {
    & $kit.InfVerif /w /v (Join-Path $OutputDirectory $infName)
    if ($LASTEXITCODE -ne 0) { throw "INF verification failed for $infName ($LASTEXITCODE)." }
}
if (-not $SkipCatalog) {
    & $kit.Inf2Cat "/driver:$OutputDirectory" /os:10_X64 /uselocaltime
    if ($LASTEXITCODE -ne 0) { throw "Catalog generation failed ($LASTEXITCODE)." }
}
$evidence = [ordered]@{
    builtUtc = [DateTime]::UtcNow.ToString('o')
    architecture = 'x64'
    umdf = '2.25'
    minimumWindowsBuild = 22000
    nativeStaticAnalysis = 'MSVC /analyze /W4 /WX passed; WDK-header-only warnings scoped out.'
    nativeAbiCheck = 'PASS: packet validation, output bounds, neutral state, descriptor report lengths.'
    signing = 'Unsigned; locally sign and trust only through the explicit installer.'
    compiler = $kit.Cl
    wdkRoot = $kit.WdkRoot
    rustCoreSha256 = (Get-FileHash -LiteralPath $RustLibrary -Algorithm SHA256).Hash
    files = @(Get-ChildItem -LiteralPath $OutputDirectory -File | Where-Object Extension -In '.dll','.inf','.cat' | ForEach-Object {
        @{ name = $_.Name; bytes = $_.Length; sha256 = (Get-FileHash -LiteralPath $_.FullName -Algorithm SHA256).Hash }
    })
}
$evidence | ConvertTo-Json -Depth 5 | Set-Content -LiteralPath (Join-Path $OutputDirectory 'build-evidence.json') -Encoding utf8
Write-Output "Built SCX VirtualPad UMDF2 x64: $OutputDirectory"
