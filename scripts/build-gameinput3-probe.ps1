param([string]$HeaderDirectory)
$ErrorActionPreference='Stop'
$root=Split-Path -Parent $PSScriptRoot
if(-not $HeaderDirectory){$HeaderDirectory=[IO.Path]::GetFullPath((Join-Path $root '../../.tooling/gameinput-probe'))}
if(-not (Test-Path -LiteralPath (Join-Path $HeaderDirectory 'GameInput-3.5.262.h'))){throw 'Microsoft.GameInput 3.5.262 test-only public header is required.'}
$kit=& (Join-Path $PSScriptRoot 'check-wdk.ps1') -AsObject
$out=Join-Path $root 'target/diagnostics'
New-Item -ItemType Directory -Path $out -Force | Out-Null
$includes=@($HeaderDirectory,(Join-Path $kit.MsvcRoot 'include'))
foreach($name in @('ucrt','um','shared')) {$includes+=Join-Path $kit.SdkRoot "Include/$($kit.SdkVersion)/$name"}
$libs=@((Join-Path $kit.MsvcRoot 'lib/x64'))
foreach($name in @('ucrt','um')) {$libs+=Join-Path $kit.SdkRoot "Lib/$($kit.SdkVersion)/$name/x64"}
$compile=@('/nologo','/std:c++20','/EHsc','/O2','/MT','/W4','/WX',"/Fo$out/gameinput3_probe.obj",(Join-Path $root 'driver/gameinput3_probe.cpp'))+($includes | ForEach-Object {"/I$_"})
& $kit.Cl @compile /link "/OUT:$out/gameinput3_probe.exe" 'kernel32.lib' 'user32.lib' @($libs | ForEach-Object {"/LIBPATH:$_"})
if($LASTEXITCODE -ne 0){throw 'GameInput v3 probe build failed'}
