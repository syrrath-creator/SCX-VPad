$ErrorActionPreference='Stop'
$root=Split-Path -Parent $PSScriptRoot
$kit=& (Join-Path $PSScriptRoot 'check-wdk.ps1') -AsObject
$out=Join-Path $root 'target/diagnostics'
New-Item -ItemType Directory -Path $out -Force | Out-Null
$includes=@((Join-Path $kit.MsvcRoot 'include'))
foreach($name in @('ucrt','um','shared')) {$includes+=Join-Path $kit.SdkRoot "Include/$($kit.SdkVersion)/$name"}
$libs=@((Join-Path $kit.MsvcRoot 'lib/x64'))
foreach($name in @('ucrt','um')) {$libs+=Join-Path $kit.SdkRoot "Lib/$($kit.SdkVersion)/$name/x64"}
$compile=@('/nologo','/std:c++20','/EHsc','/O2','/MT','/W4','/WX',"/Fo$out/identity_probe.obj",(Join-Path $root 'driver/identity_probe.cpp'))+($includes | ForEach-Object {"/I$_"})
& $kit.Cl @compile /link "/OUT:$out/identity_probe.exe" 'hid.lib' 'setupapi.lib' 'cfgmgr32.lib' 'dinput8.lib' 'dxguid.lib' 'gameinput.lib' 'kernel32.lib' 'user32.lib' 'advapi32.lib' @($libs | ForEach-Object {"/LIBPATH:$_"})
if($LASTEXITCODE -ne 0){throw 'Identity probe build failed'}
