[CmdletBinding()]
param(
    [Parameter(Mandatory=$true)][string]$PackageZip,
    [Parameter(Mandatory=$true)][ValidatePattern('^[a-fA-F0-9]{64}$')][string]$ExpectedSha256,
    [Parameter(Mandatory=$true)][string]$OutputDirectory
)
$ErrorActionPreference = 'Stop'
$root = Split-Path -Parent $PSScriptRoot
$PackageZip = (Resolve-Path -LiteralPath $PackageZip).Path
$OutputDirectory = [IO.Path]::GetFullPath($OutputDirectory)
if ((Get-FileHash -LiteralPath $PackageZip -Algorithm SHA256).Hash -ne $ExpectedSha256) { throw 'Release ZIP checksum mismatch.' }
$compiler = Join-Path ([Environment]::GetFolderPath('Windows')) 'Microsoft.NET\Framework64\v4.0.30319\csc.exe'
if (-not (Test-Path -LiteralPath $compiler)) { throw 'The Windows .NET Framework C# compiler is required.' }
New-Item -ItemType Directory -Path $OutputDirectory -Force | Out-Null
$buildInfo = Join-Path $OutputDirectory 'BuildInfo.cs'
$code = @"
using System.Reflection;
[assembly: AssemblyTitle("SCX VPad Setup")]
[assembly: AssemblyDescription("SCX VPad driver and service installer")]
[assembly: AssemblyCompany("Tim Haller")]
[assembly: AssemblyProduct("SCX VPad")]
[assembly: AssemblyCopyright("Copyright 2026 Tim Haller")]
[assembly: AssemblyVersion("1.0.0.0")]
[assembly: AssemblyFileVersion("1.0.0.0")]
namespace ScxSetup { internal static class BuildInfo {
internal const string DriverVersion = "0.1.0.28";
internal const string PayloadSha256 = "$($ExpectedSha256.ToLowerInvariant())";
} }
"@
[IO.File]::WriteAllText($buildInfo, $code, (New-Object Text.UTF8Encoding($false)))
$exe = Join-Path $OutputDirectory 'SCX-VPad-0.1.0.28-Setup-x64.exe'
$source = Join-Path $root 'installer\gui\Setup.cs'
$refs = @('/reference:System.dll','/reference:System.Core.dll','/reference:System.Drawing.dll','/reference:System.Windows.Forms.dll','/reference:System.IO.Compression.dll','/reference:System.IO.Compression.FileSystem.dll','/reference:System.Web.Extensions.dll')
& $compiler /nologo /target:winexe /platform:x64 /optimize+ /warnaserror+ /utf8output "/out:$exe" "/win32manifest:$root\installer\gui\setup.manifest" "/resource:$PackageZip,SCX.Payload.zip" "/resource:$root\installer\gui\Desktop.ps1,SCX.Desktop.ps1" @refs $source $buildInfo
if ($LASTEXITCODE -ne 0) { throw 'Setup compilation failed.' }
$sum = (Get-FileHash -LiteralPath $exe -Algorithm SHA256).Hash.ToLowerInvariant() + '  ' + [IO.Path]::GetFileName($exe) + "`n"
[IO.File]::WriteAllText((Join-Path $OutputDirectory 'SCX-VPad-0.1.0.28-Setup-SHA256SUMS.txt'), $sum, (New-Object Text.UTF8Encoding($false)))
Write-Output $exe
Write-Output 'The published driver ZIP is embedded unchanged. The Setup EXE itself is not publisher-signed.'
