[CmdletBinding()]
param([Parameter(Mandatory=$true)][string]$PackageZip, [Parameter(Mandatory=$true)][string]$BuildDirectory)
$ErrorActionPreference = 'Stop'
$root = Split-Path -Parent $PSScriptRoot
$PackageZip = (Resolve-Path -LiteralPath $PackageZip).Path
$BuildDirectory = (Resolve-Path -LiteralPath $BuildDirectory).Path
$compiler = Join-Path ([Environment]::GetFolderPath('Windows')) 'Microsoft.NET\Framework64\v4.0.30319\csc.exe'
$testExe = Join-Path $BuildDirectory 'SetupTests.exe'
$refs = @('/reference:System.dll','/reference:System.Core.dll','/reference:System.Drawing.dll','/reference:System.Windows.Forms.dll','/reference:System.IO.Compression.dll','/reference:System.IO.Compression.FileSystem.dll','/reference:System.Web.Extensions.dll')
& $compiler /nologo /target:exe /main:ScxSetup.SetupTests /platform:x64 /optimize+ /warnaserror+ /utf8output "/out:$testExe" "/resource:$PackageZip,SCX.Payload.zip" "/resource:$root\installer\gui\Desktop.ps1,SCX.Desktop.ps1" @refs (Join-Path $root 'installer\gui\Setup.cs') (Join-Path $root 'installer\gui\SetupTests.cs') (Join-Path $BuildDirectory 'BuildInfo.cs')
if ($LASTEXITCODE -ne 0) { throw 'Setup tests failed to compile.' }
& $testExe
if ($LASTEXITCODE -ne 0) { throw 'Setup tests failed.' }
$tokens = $null; $errors = $null
[Management.Automation.Language.Parser]::ParseFile((Join-Path $root 'installer\gui\Desktop.ps1'), [ref]$tokens, [ref]$errors) | Out-Null
if ($errors.Count) { throw ($errors | Out-String) }
Write-Output 'PASS: PowerShell operation wrapper syntax.'
& (Join-Path $PSScriptRoot 'test-setup-identity.ps1')
