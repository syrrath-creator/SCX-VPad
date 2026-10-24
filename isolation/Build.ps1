$ErrorActionPreference='Stop'
$repo=Split-Path -Parent $PSScriptRoot
$kit=& (Join-Path $repo 'scripts/check-wdk.ps1') -AsObject
$manifest=Join-Path $PSScriptRoot 'policy/Cargo.toml'
& (Join-Path $repo 'scripts/cargo.ps1') -CargoArgs @('test','--manifest-path',$manifest,'--offline')
& (Join-Path $repo 'scripts/cargo.ps1') -CargoArgs @('clippy','--manifest-path',$manifest,'--offline','--all-targets','--','-D','warnings')
& (Join-Path $repo 'scripts/cargo.ps1') -CargoArgs @('build','--manifest-path',$manifest,'--offline','--release','--no-default-features')
$out=Join-Path $PSScriptRoot 'build'
New-Item -ItemType Directory -Path $out -Force|Out-Null
$includes=@((Join-Path $repo 'include'),(Join-Path $kit.WdkRoot 'Include/wdf/umdf/2.25'),(Join-Path $kit.MsvcRoot 'include'))
foreach($part in @('ucrt','um','shared')){$includes+=Join-Path $kit.SdkRoot "Include/$($kit.SdkVersion)/$part"}
foreach($part in @('shared','um','km')){$includes+=Join-Path $kit.WdkRoot "Include/$($kit.SdkVersion)/$part"}
$libs=@((Join-Path $kit.MsvcRoot 'lib/x64'),(Join-Path $kit.SdkRoot "Lib/$($kit.SdkVersion)/ucrt/x64"),(Join-Path $kit.SdkRoot "Lib/$($kit.SdkVersion)/um/x64"))
$object=Join-Path $out 'scxsource.obj'
$args=@('/nologo','/c','/TC','/std:c17','/O2','/MT','/GS','/guard:cf','/analyze','/analyze:external-','/W4','/WX','/DWIN32_LEAN_AND_MEAN','/DUNICODE','/D_UNICODE','/DUMDF_VERSION_MAJOR=2','/DUMDF_VERSION_MINOR=25','/DUMDF_USING_NTSTATUS',"/Fo$object",(Join-Path $PSScriptRoot 'native/scxsource.c'))+@($includes|ForEach-Object{"/I$_"})
& $kit.Cl @args
if($LASTEXITCODE){throw 'Source-filter compilation failed'}
$resource=Join-Path $out 'ScxSource.res'
& $kit.Rc /nologo "/fo$resource" @($includes|ForEach-Object{"/I$_"}) (Join-Path $PSScriptRoot 'native/ScxSource.rc')
if($LASTEXITCODE){throw 'Source-filter version resource failed'}
$args=@('/nologo','/WX','/DLL','/MACHINE:X64','/SUBSYSTEM:WINDOWS,10.00','/DYNAMICBASE','/NXCOMPAT','/HIGHENTROPYVA','/guard:cf','/CETCOMPAT','/OPT:REF','/OPT:ICF','/INCREMENTAL:NO','/Brepro',('/DEF:'+(Join-Path $PSScriptRoot 'native/ScxSource.def')),('/OUT:'+(Join-Path $out 'ScxSource.dll')),$object,(Join-Path $PSScriptRoot 'policy/target/release/scx_source_policy.lib'),$kit.WdfLibrary,'kernel32.lib','ntdll.lib','advapi32.lib','bcrypt.lib')+@($libs|ForEach-Object{"/LIBPATH:$_"})
& $kit.Link @args $resource
if($LASTEXITCODE){throw 'Source-filter linking failed'}
Copy-Item -LiteralPath (Join-Path $PSScriptRoot 'native/scxsource.inf') -Destination $out -Force
& $kit.InfVerif /w /v (Join-Path $out 'scxsource.inf')
if($LASTEXITCODE){throw 'Source-filter INF validation failed'}
& $kit.Inf2Cat "/driver:$out" /os:10_X64 /uselocaltime
if($LASTEXITCODE){throw 'Source-filter catalog failed'}
[ordered]@{scope='Device-specific Edge Bluetooth filter; build checks only; installed results recorded separately';files=@(Get-ChildItem $out -File|Where-Object Extension -in '.dll','.inf','.cat'|ForEach-Object{@{name=$_.Name;sha256=(Get-FileHash $_.FullName).Hash}})}|ConvertTo-Json -Depth 5|Set-Content (Join-Path $out 'build.json') -Encoding utf8
