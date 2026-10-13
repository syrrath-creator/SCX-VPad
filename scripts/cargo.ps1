param([Parameter(ValueFromRemainingArguments=$true)][string[]]$CargoArgs)
$ErrorActionPreference = 'Stop'
$projectRoot = Split-Path -Parent $PSScriptRoot
$existingToolchain = Join-Path (Split-Path -Parent $projectRoot) 'scx-rust/.tooling'
$cargoCommand = Get-Command cargo -ErrorAction SilentlyContinue
if (Test-Path -LiteralPath (Join-Path $existingToolchain 'cargo/bin/cargo.exe')) {
    $env:RUSTUP_HOME = Join-Path $existingToolchain 'rustup'
    $env:CARGO_HOME = Join-Path $existingToolchain 'cargo'
    $env:PATH = (Join-Path $env:CARGO_HOME 'bin') + ';' + $env:PATH
    $cargoExe = Join-Path $env:CARGO_HOME 'bin/cargo.exe'
} elseif ($cargoCommand) {
    $cargoExe = $cargoCommand.Source
} else { throw 'Install the stable Rust MSVC toolchain before building.' }
Push-Location $projectRoot
try {
    & $cargoExe @CargoArgs
    if ($LASTEXITCODE -ne 0) { throw "Cargo failed with exit code $LASTEXITCODE" }
} finally { Pop-Location }

