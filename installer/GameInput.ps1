# Windows Gaming Input mappings, authored from Microsoft's public schema:
# https://learn.microsoft.com/windows/uwp/gaming/registry-data-for-game-controllers
# These mappings are VID/PID-wide. They also describe native Sony USB reports.
# Only the two emulated USB identities are touched; Edge/Bluetooth are excluded.
Set-StrictMode -Version Latest

function Get-ScxSonyMappings {
    $leaves = [ordered]@{}
    $buttons = [ordered]@{ A=1; B=2; X=0; Y=3; LeftShoulder=4; RightShoulder=5; View=8; Menu=9; LeftThumbstickButton=10; RightThumbstickButton=11 }
    foreach ($name in $buttons.Keys) { $leaves["Gamepad\$name"] = @{ ButtonIndex=[int]$buttons[$name] } }
    # RawGameController orders HID axes by usage: X,Y,Z,Rx,Ry,Rz.
    $axes = [ordered]@{ LeftThumbstickX=0; LeftThumbstickY=1; RightThumbstickX=2; RightThumbstickY=5; LeftTrigger=3; RightTrigger=4 }
    foreach ($name in $axes.Keys) {
        $values = @{ AxisIndex=[int]$axes[$name] }
        if ($name.EndsWith('Y')) { $values.Invert = 1 }
        $leaves["Gamepad\$name"] = $values
    }
    foreach ($direction in @('Up','Down','Left','Right')) {
        $values = @{ SwitchIndex=0; SwitchPosition=$direction; IncludeAdjacent=1 }
        $leaves["Gamepad\DPad$direction"] = $values
        $leaves["UINavigation\Primary$direction"] = $values
    }
    foreach ($entry in @(@('Accept',1),@('Cancel',2),@('View',8),@('Menu',9))) {
        $leaves['UINavigation\' + $entry[0]] = @{ ButtonIndex=[int]$entry[1] }
    }
    return $leaves
}

function Get-ScxRegistryValue($Key, [string]$Name) {
    if ($null -eq $Key -or $Key.GetValueNames() -notcontains $Name) { return $null }
    return [ordered]@{ kind=$Key.GetValueKind($Name).ToString(); data=$Key.GetValue($Name, $null, [Microsoft.Win32.RegistryValueOptions]::DoNotExpandEnvironmentNames) }
}

function Set-ScxRegistryValue($Key, [string]$Name, $Value) {
    if ($null -eq $Value) { $Key.DeleteValue($Name, $false); return }
    $kind = [Microsoft.Win32.RegistryValueKind]$Value.kind
    $data = switch ($kind) {
        DWord { [int]$Value.data }
        QWord { [long]$Value.data }
        Binary { ,[byte[]]$Value.data }
        MultiString { ,[string[]]$Value.data }
        default { [string]$Value.data }
    }
    $Key.SetValue($Name, $data, $kind)
}

function Test-ScxRegistryValueEqual($A, $B) {
    return (ConvertTo-Json -InputObject $A -Depth 8 -Compress) -ceq (ConvertTo-Json -InputObject $B -Depth 8 -Compress)
}

function Install-ScxGameInputMappings {
    param([Microsoft.Win32.RegistryKey]$MachineRoot = [Microsoft.Win32.Registry]::LocalMachine)
    $product = $MachineRoot.OpenSubKey('SOFTWARE\SCX\VirtualPad', $true)
    if ($null -eq $product) { throw 'A protected SCX VPad installation record is required.' }
    try {
        if ($product.GetValueNames() -contains 'GameInputJournal') { throw 'GameInput mappings have already been journaled; restore them before reinstalling.' }
        $base = 'SYSTEM\CurrentControlSet\Control\GameInput\Devices'
        $mappings = Get-ScxSonyMappings
        $fields = @('ButtonIndex','AxisIndex','Invert','SwitchIndex','SwitchPosition','IncludeAdjacent','ThresholdPercent','DebouncePercent','MaxValueButtonIndex','MinValueButtonIndex','MaxValueSwitchPosition')
        $entries = @()
        $created = @{}
        foreach ($id in @('054C09CC00010005','054C0CE600010005')) {
            foreach ($leaf in $mappings.Keys) {
                $path = "$base\$id\$leaf"
                # Journal missing key ancestors so uninstall removes empty keys only.
                foreach ($relative in @($id, ($id + '\' + ($leaf -split '\\')[0]), ($id + '\' + $leaf))) {
                    $keyPath = "$base\$relative"
                    $existing = $MachineRoot.OpenSubKey($keyPath)
                    if ($null -eq $existing) { $created[$keyPath] = $true } else { $existing.Dispose() }
                }
                $key = $MachineRoot.OpenSubKey($path)
                try {
                    foreach ($field in $fields) {
                        $before = Get-ScxRegistryValue $key $field
                        $after = if ($mappings[$leaf].ContainsKey($field)) {
                            $value = $mappings[$leaf][$field]
                            [ordered]@{ kind=$(if ($value -is [string]) { 'String' } else { 'DWord' }); data=$value }
                        } else { $null }
                        if (-not (Test-ScxRegistryValueEqual $before $after)) {
                            $entries += [ordered]@{ path=$path; name=$field; before=$before; after=$after }
                        }
                    }
                } finally { if ($key) { $key.Dispose() } }
            }
        }
        # Persist recovery data BEFORE the first write, including partial setup.
        $journal = [ordered]@{ version=1; entries=@($entries); created=@($created.Keys | Sort-Object) }
        $product.SetValue('GameInputJournal', (ConvertTo-Json -InputObject $journal -Depth 10 -Compress), [Microsoft.Win32.RegistryValueKind]::String)
        foreach ($entry in $entries) {
            $key = $MachineRoot.CreateSubKey($entry.path)
            try { Set-ScxRegistryValue $key $entry.name $entry.after } finally { $key.Dispose() }
        }
        $product.SetValue('GameInputMappingVersion', 1, [Microsoft.Win32.RegistryValueKind]::DWord)
    } finally { $product.Dispose() }
}

function Restore-ScxGameInputMappings {
    param([Microsoft.Win32.RegistryKey]$MachineRoot = [Microsoft.Win32.Registry]::LocalMachine)
    $product = $MachineRoot.OpenSubKey('SOFTWARE\SCX\VirtualPad', $true)
    if ($null -eq $product) { throw 'No SCX VPad installation record.' }
    try {
        if ($product.GetValueNames() -notcontains 'GameInputJournal') { return }
        $journal = $product.GetValue('GameInputJournal') | ConvertFrom-Json
        if ($journal.version -ne 1) { throw 'Unknown GameInput journal version.' }
        $base = 'SYSTEM\CurrentControlSet\Control\GameInput\Devices'
        $allowed = @{}
        foreach ($id in @('054C09CC00010005','054C0CE600010005')) {
            $allowed["$base\$id"] = $true
            foreach ($leaf in (Get-ScxSonyMappings).Keys) {
                $allowed["$base\$id\$leaf"] = $true
                $allowed["$base\$id\" + ($leaf -split '\\')[0]] = $true
            }
        }
        foreach ($entry in $journal.entries) {
            if (-not $allowed.ContainsKey([string]$entry.path) -or $entry.name -notmatch '^(ButtonIndex|AxisIndex|Invert|SwitchIndex|SwitchPosition|IncludeAdjacent|ThresholdPercent|DebouncePercent|MaxValueButtonIndex|MinValueButtonIndex|MaxValueSwitchPosition)$') { throw 'Unexpected GameInput journal path or field.' }
        }
        foreach ($path in $journal.created) { if (-not $allowed.ContainsKey([string]$path)) { throw 'Unexpected GameInput cleanup path.' } }
        foreach ($entry in $journal.entries) {
            $key = $MachineRoot.OpenSubKey($entry.path, $true)
            try {
                $current = Get-ScxRegistryValue $key $entry.name
                if (Test-ScxRegistryValueEqual $current $entry.before) { continue }
                if (-not (Test-ScxRegistryValueEqual $current $entry.after)) {
                    # A later installer/user owns this value now; keep their edit.
                    Write-Warning "Preserving externally changed GameInput value: $($entry.path)\$($entry.name)"
                    continue
                }
                if ($null -eq $key) { $key = $MachineRoot.CreateSubKey($entry.path) }
                Set-ScxRegistryValue $key $entry.name $entry.before
            } finally { if ($key) { $key.Dispose() } }
        }
        foreach ($path in ($journal.created | Sort-Object Length -Descending)) {
            $key = $MachineRoot.OpenSubKey($path)
            if ($null -eq $key) { continue }
            try { $empty = $key.SubKeyCount -eq 0 -and $key.ValueCount -eq 0 } finally { $key.Dispose() }
            # DeleteSubKey cannot remove a tree and fails if a subkey appeared.
            if ($empty) { $MachineRoot.DeleteSubKey($path, $false) }
        }
        $product.DeleteValue('GameInputJournal', $false)
        $product.DeleteValue('GameInputMappingVersion', $false)
    } finally { $product.Dispose() }
}
