$ErrorActionPreference='Stop'
. (Join-Path $PSScriptRoot 'SourceModels.ps1')
$template=[IO.File]::ReadAllText((Join-Path $PSScriptRoot 'native/scxsource.inf'))
$model='HID\VID_046D&PID_C24F&REV_0100&MI_00&COL01'
$result=Add-ScxSourceModels $template @($model,$model.ToLowerInvariant())
if (($result.Split(@('%Device%=Source,'+$model),[StringSplitOptions]::None)).Count -ne 2) { throw 'Exact model must occur once.' }
foreach($bad in @('HID\*','HID_DEVICE_SYSTEM_GAME','HID_DEVICE_UP:0001_U:0005','HID\VID_054C&PID_0CE6\SERIAL','HID\VID_045E&PID_028E&IG_00',"HID\VID_054C&PID_0CE6`r`n[Injected]",'USB\VID_054C&PID_0CE6','HID\VID_054C&PID_0CE6,OtherSection')) {
    $rejected=$false
    try { $null=Add-ScxSourceModels $template @($bad) } catch { $rejected=$true }
    if(-not $rejected) { throw 'Unsafe model accepted.' }
}
Write-Output 'PASS: exact model deduplication; wildcard, reserved class/usage, XInput, instance and INF injection rejected.'
foreach($model in @('HID\VID_054C&PID_0CE6','HID\VID_054C&PID_09CC','HID\VID_046D&PID_C24F&COL01')) {
    $rejected=$false
    try { $null=Add-ScxSourceModels $template @($model) } catch { $rejected=$true }
    if(-not $rejected){throw 'Revision-less USB model could also capture a virtual output.'}
    if($template.Contains('%Device%=Source,'+$model+"`r`n") -or $template.Contains('%Device%=Source,'+$model+"`n")){throw 'Broad USB model in base INF.'}
}
Write-Output 'PASS: broad USB IDs that can bind virtual output nodes excluded.'
