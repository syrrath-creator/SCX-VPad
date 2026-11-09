[CmdletBinding()]
param([string]$OutputDirectory = (Join-Path $PSScriptRoot 'build'))
$ErrorActionPreference = 'Stop'
$kit = & (Join-Path $PSScriptRoot '../scripts/check-wdk.ps1') -AsObject
$out = [IO.Path]::GetFullPath($OutputDirectory)
New-Item -ItemType Directory -Path $out -Force | Out-Null
$includes = @((Join-Path $kit.MsvcRoot 'include'))
foreach ($part in @('ucrt','um','shared')) { $includes += Join-Path $kit.SdkRoot "Include/$($kit.SdkVersion)/$part" }
$libs = @((Join-Path $kit.MsvcRoot 'lib/x64'))
foreach ($part in @('ucrt','um')) { $libs += Join-Path $kit.SdkRoot "Lib/$($kit.SdkVersion)/$part/x64" }
$compile = @('/nologo','/TC','/std:c17','/O2','/MT','/W4','/WX','/GS','/guard:cf',
    ('/Fe:' + (Join-Path $out 'dinput-probe.exe')), ('/Fo:' + (Join-Path $out 'dinput_probe.obj')),
    (Join-Path $PSScriptRoot 'native/dinput_probe.c')) + @($includes | ForEach-Object { "/I$_" })
& $kit.Cl @compile /link /DYNAMICBASE /NXCOMPAT /CETCOMPAT /guard:cf dinput8.lib dxguid.lib ole32.lib user32.lib kernel32.lib @($libs | ForEach-Object { "/LIBPATH:$_" })
if ($LASTEXITCODE -ne 0) { throw 'Bounded DirectInput probe build failed.' }
Write-Output (Join-Path $out 'dinput-probe.exe')
