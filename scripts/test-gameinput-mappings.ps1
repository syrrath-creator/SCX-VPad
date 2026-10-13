$ErrorActionPreference = 'Stop'
Set-StrictMode -Version Latest
. (Join-Path $PSScriptRoot '../installer/GameInput.ps1')
# Exercise real registry types in an isolated, unprivileged fixture. No HKLM writes.
$fixture = 'SOFTWARE\SCX-GameInput-Test-' + [Guid]::NewGuid().ToString('N')
$root = [Microsoft.Win32.Registry]::CurrentUser.CreateSubKey($fixture)
try {
    $product = $root.CreateSubKey('SOFTWARE\SCX\VirtualPad'); $product.Dispose()
    $base = 'SYSTEM\CurrentControlSet\Control\GameInput\Devices'
    $old = $root.CreateSubKey("$base\054C0CE600010005\Gamepad\A")
    $old.SetValue('ButtonIndex',0,[Microsoft.Win32.RegistryValueKind]::DWord)
    $old.SetValue('VendorNote','preserve me'); $old.Dispose()
    Install-ScxGameInputMappings -MachineRoot $root
    foreach ($id in @('054C09CC00010005','054C0CE600010005')) {
        foreach ($check in @(@('Gamepad\A','ButtonIndex',1),@('Gamepad\B','ButtonIndex',2),@('Gamepad\X','ButtonIndex',0),@('Gamepad\RightThumbstickY','AxisIndex',5),@('Gamepad\LeftTrigger','AxisIndex',3),@('Gamepad\RightTrigger','AxisIndex',4),@('UINavigation\Accept','ButtonIndex',1))) {
            $key = $root.OpenSubKey("$base\$id\$($check[0])")
            try { if ($key.GetValue($check[1]) -ne $check[2]) { throw "Incorrect mapping $id $($check[0])" } } finally { $key.Dispose() }
        }
    }
    $blocked = $false
    try { Install-ScxGameInputMappings -MachineRoot $root } catch { $blocked = $true }
    if (-not $blocked) { throw 'Repeated installation overwrote recovery journal.' }
    # A different program changes one value after SCX. Do not overwrite it.
    $changed = $root.OpenSubKey("$base\054C0CE600010005\Gamepad\B", $true)
    $changed.SetValue('ButtonIndex',42,[Microsoft.Win32.RegistryValueKind]::DWord); $changed.Dispose()
    Restore-ScxGameInputMappings -MachineRoot $root
    $old = $root.OpenSubKey("$base\054C0CE600010005\Gamepad\A")
    try { if ($old.GetValue('ButtonIndex') -ne 0 -or $old.GetValue('VendorNote') -ne 'preserve me') { throw 'Original mapping was not restored.' } } finally { $old.Dispose() }
    $changed = $root.OpenSubKey("$base\054C0CE600010005\Gamepad\B")
    try { if ($changed.GetValue('ButtonIndex') -ne 42) { throw 'Third-party change was overwritten.' } } finally { $changed.Dispose() }
    $removed = $root.OpenSubKey("$base\054C09CC00010005")
    if ($removed) { $removed.Dispose(); throw 'Empty owned identity was not cleaned up.' }
    Restore-ScxGameInputMappings -MachineRoot $root
    'PASS: Sony mappings, existing-value recovery, external changes, empty-key cleanup and idempotent removal.'
} finally {
    $root.Dispose()
    if ($fixture -notmatch '^SOFTWARE\\SCX-GameInput-Test-[0-9a-f]{32}$') { throw 'Unexpected test fixture path.' }
    [Microsoft.Win32.Registry]::CurrentUser.DeleteSubKeyTree($fixture)
}
