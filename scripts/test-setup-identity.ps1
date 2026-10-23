[CmdletBinding()]
param([switch]$RequireWindowsPowerShell51)
$ErrorActionPreference='Stop'
Set-StrictMode -Version Latest
if($RequireWindowsPowerShell51 -and ($PSVersionTable.PSEdition -ne 'Desktop' -or $PSVersionTable.PSVersion.Major -ne 5 -or $PSVersionTable.PSVersion.Minor -ne 1)){throw 'Run this check in Windows PowerShell 5.1.'}
$source=Join-Path (Split-Path -Parent $PSScriptRoot) 'installer\gui\Desktop.ps1'
$tokens=$null;$errors=$null
$ast=[Management.Automation.Language.Parser]::ParseFile($source,[ref]$tokens,[ref]$errors)
if($errors.Count){throw ($errors | Out-String)}
# Load only these two functions. Never execute the install/remove entry point.
foreach($name in @('Get-ValidatedReplacementIdentity','Restore-ReplacementIdentity')){
    $definitions=@($ast.FindAll({param($node) $node -is [Management.Automation.Language.FunctionDefinitionAst] -and $node.Name -eq $name},$true))
    if($definitions.Count -ne 1){throw ('Expected exactly one '+$name)}
    . ([ScriptBlock]::Create($definitions[0].Extent.Text))
}
$script:checks=0
function Check([bool]$Ok,[string]$Label){if(-not $Ok){throw ('FAIL: '+$Label)};$script:checks++;Write-Output ('PASS: '+$Label)}
function Rejected([scriptblock]$Action){try{& $Action | Out-Null;return $false}catch{return $true}}
function Record($Seed){[pscustomobject]@{InstallDirectory=(Join-Path ([Environment]::GetFolderPath([Environment+SpecialFolder]::ProgramFiles)) 'SCX VPad');InstallOwnerSid='S-1-5-21-1-2-3-1001';DeviceIdentitySeed=$Seed}}
$qword=[Microsoft.Win32.RegistryValueKind]::QWord
foreach($seed in @([Int64]1,[Int64]::MaxValue,[Int64]::MinValue,[Int64](-1),[UInt64]::MaxValue,[UInt64]::Parse('9223372036854775808'))){
    $identity=Get-ValidatedReplacementIdentity (Record $seed) $qword
    $expected=if($seed -is [UInt64]){[BitConverter]::ToString([BitConverter]::GetBytes([UInt64]$seed))}else{[BitConverter]::ToString([BitConverter]::GetBytes([Int64]$seed))}
    Check ([BitConverter]::ToString([BitConverter]::GetBytes([Int64]$identity.Seed)) -eq $expected) ('QWORD bits preserved: '+$seed)
}
foreach($seed in @([Int64]0,[UInt64]0,[Int32]1,'123',[double]123,$null)){
    Check (Rejected {Get-ValidatedReplacementIdentity (Record $seed) $qword}) ('Reject zero/non-QWORD value: '+[string]$seed)
}
Check (Rejected {Get-ValidatedReplacementIdentity (Record ([Int64]1)) ([Microsoft.Win32.RegistryValueKind]::DWord)}) 'Reject non-QWORD registry type'
$bad=Record ([Int64]1);$bad.InstallDirectory='C:\Other\SCX VPad'
Check (Rejected {Get-ValidatedReplacementIdentity $bad $qword}) 'Reject unrelated installation path'
$bad=Record ([Int64]1);$bad.InstallOwnerSid="S-1-5-21-1-2-3-1001`n"
Check (Rejected {Get-ValidatedReplacementIdentity $bad $qword}) 'Reject malformed recorded owner'
$bad=Record ([Int64]1);$bad.PSObject.Properties.Remove('DeviceIdentitySeed')
Check (Rejected {Get-ValidatedReplacementIdentity $bad $qword}) 'Reject missing recorded seed before removal'

# Every Windows-facing command used by the restore function is shadowed below.
# These mocks reject any other key/service and never access HKLM or the SCM.
$script:stored=Record ([Int64]17)
$script:kind=$qword
$script:events=New-Object 'Collections.Generic.List[string]'
$script:state='Running'
$script:corruptWrite=$false
function Assert-Key([string]$Path){if($Path -ne 'HKLM:\SOFTWARE\SCX\VirtualPad'){throw 'Unexpected registry path in test.'}}
function Get-ItemProperty {param([string]$LiteralPath) Assert-Key $LiteralPath;return $script:stored}
function Get-Item {
    param([string]$LiteralPath)
    Assert-Key $LiteralPath
    $item=[pscustomobject]@{}
    $item | Add-Member ScriptMethod GetValueKind {param($name) if($name -ne 'DeviceIdentitySeed'){throw 'Unexpected registry value.'};return $script:kind}
    return $item
}
function New-ItemProperty {
    param([string]$LiteralPath,[string]$Name,[string]$PropertyType,$Value,[switch]$Force)
    Assert-Key $LiteralPath
    if($Name -ne 'DeviceIdentitySeed' -or $PropertyType -ne 'QWord' -or -not $Force -or $Value -isnot [Int64] -or $script:state -ne 'Stopped'){throw 'Invalid/unordered seed write.'}
    $script:events.Add('Write')
    $script:stored.DeviceIdentitySeed=if($script:corruptWrite){[Int64]123}else{$Value}
}
function Get-Service {
    [CmdletBinding()]param([string]$Name)
    if($Name -ne 'ScxVPad'){throw 'Unexpected service.'}
    $service=[pscustomobject]@{Status=$script:state}
    $service | Add-Member ScriptMethod WaitForStatus {param($status,$timeout) if($status -ne $script:state -or $timeout.TotalSeconds -ne 25){throw 'Wrong service wait.'};$script:events.Add('Wait:'+$status)}
    return $service
}
function Stop-Service {[CmdletBinding()]param([string]$Name) if($Name -ne 'ScxVPad'){throw 'Unexpected stop.'};$script:events.Add('Stop');$script:state='Stopped'}
function Start-Service {[CmdletBinding()]param([string]$Name) if($Name -ne 'ScxVPad'){throw 'Unexpected start.'};$script:events.Add('Start');$script:state='Running'}
$identity=Get-ValidatedReplacementIdentity (Record ([UInt64]::MaxValue)) $qword
Restore-ReplacementIdentity $identity
Check (($script:events -join ',') -eq 'Stop,Wait:Stopped,Write,Start,Wait:Running' -and $script:stored.DeviceIdentitySeed -eq [Int64](-1) -and $script:stored.InstallOwnerSid -eq $identity.OwnerSid) 'Restore original bits and owner with ordered stop/write/read-back/start'
$script:stored=Record ([Int64]17);$script:events.Clear();$script:corruptWrite=$true
Check (Rejected {Restore-ReplacementIdentity $identity}) 'Reject failed seed read-back'
Check ($script:state -eq 'Stopped' -and -not $script:events.Contains('Start')) 'Failed read-back cannot restart with a changed identity'
$script:stored=Record ([Int64]17);$script:stored.InstallOwnerSid='S-1-5-21-4-5-6-1001';$script:events.Clear();$script:state='Running';$script:corruptWrite=$false
Check (Rejected {Restore-ReplacementIdentity $identity}) 'Reject changed authorized owner'
Check ($script:events.Count -eq 0) 'Wrong owner fails before any service/registry mutation'
Write-Output ('Setup identity: '+$script:checks+' passed on PowerShell '+$PSVersionTable.PSVersion+'. No real registry, service, driver or trust changes.')
