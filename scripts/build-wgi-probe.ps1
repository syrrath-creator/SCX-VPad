$ErrorActionPreference='Stop'
$root=Split-Path -Parent $PSScriptRoot
$kit=& (Join-Path $PSScriptRoot 'check-wdk.ps1') -AsObject
$out=Join-Path $root 'target/diagnostics'
New-Item -ItemType Directory -Path $out -Force | Out-Null
$includePaths=@((Join-Path $kit.MsvcRoot 'include'))
foreach($name in @('ucrt','um','shared','winrt','cppwinrt')) {$includePaths+=Join-Path $kit.SdkRoot "Include/$($kit.SdkVersion)/$name"}
$libPaths=@((Join-Path $kit.MsvcRoot 'lib/x64'))
foreach($name in @('ucrt','um')) {$libPaths+=Join-Path $kit.SdkRoot "Lib/$($kit.SdkVersion)/$name/x64"}
$compile=@('/nologo','/std:c++20','/EHsc','/O2','/MT','/W4',"/Fo$out/wgi_probe.obj",(Join-Path $root 'driver/wgi_probe.cpp'))+($includePaths | ForEach-Object {"/I$_"})
& $kit.Cl @compile /link "/OUT:$out/wgi_probe.exe" 'windowsapp.lib' 'runtimeobject.lib' 'kernel32.lib' 'user32.lib' 'gdi32.lib' @($libPaths | ForEach-Object {"/LIBPATH:$_"})
if($LASTEXITCODE -ne 0){throw 'WGI probe build failed'}
