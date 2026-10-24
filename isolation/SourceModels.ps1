# Pure, bounded INF-model generation. Never stages a driver or changes devices.
Set-StrictMode -Version Latest
function Add-ScxSourceModels {
    param([Parameter(Mandatory)][string]$Template, [string[]]$Models = @())
    if ($Models.Count -gt 512) { throw 'Too many source models.' }
    $section = '[SCX.NTamd64.10.0...22000]'
    if (($Template.Split(@($section),[StringSplitOptions]::None)).Count -ne 2) { throw 'Unexpected source INF template.' }
    $entries = [Collections.Generic.SortedSet[string]]::new([StringComparer]::OrdinalIgnoreCase)
    foreach ($rawModel in $Models) {
        $model = $rawModel.Trim().ToUpperInvariant()
        if ($model.Length -gt 240 -or $model -notmatch '\AHID\\(?:VID_[0-9A-F]{4}&PID_[0-9A-F]{4}|\{00001124-0000-1000-8000-00805F9B34FB\}_VID&[0-9A-F]{8}_PID&[0-9A-F]{4})(?:&(?:REV_[0-9A-F]{4}|MI_[0-9A-F]{2}|COL[0-9A-F]{2}))*\z') {
            throw 'Source model must be an exact hardware ID; wildcard, usage/class IDs and instance paths are forbidden.'
        }
        if ($model.StartsWith('HID\VID_') -and $model -notmatch '&REV_[0-9A-F]{4}(?:&|$)') {
            throw 'USB source models must retain their physical revision; broad VID/PID matches also attach to virtual gamepads.'
        }
        $alreadyPresent = @($Template -split '\r?\n' | Where-Object { $_ -ieq ('%Device%=Source,'+$model) }).Count -gt 0
        if (-not $alreadyPresent) { [void]$entries.Add($model) }
    }
    $lines = @($entries | ForEach-Object { '%Device%=Source,' + $_ })
    if (-not $lines.Count) { return $Template }
    $Template.Replace($section, $section + "`r`n" + ($lines -join "`r`n"))
}
