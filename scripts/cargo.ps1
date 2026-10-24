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
$previousEncodedFlags = $env:CARGO_ENCODED_RUSTFLAGS
if ($CargoArgs -contains '--release' -or $CargoArgs -contains 'release') {
    # Retain relative diagnostic locations, never the release builder's account path.
    $releaseFlags = @()
    if ($previousEncodedFlags) { $releaseFlags += $previousEncodedFlags -split [char]31 }
    elseif ($env:RUSTFLAGS) { $releaseFlags += $env:RUSTFLAGS -split '\s+' | Where-Object { $_ } }
    $prefixes = @(
        @([Environment]::GetFolderPath('UserProfile'), '/build-user'),
        @((Split-Path -Parent $projectRoot), '/scx-work'),
        @($env:CARGO_HOME, '/cargo'),
        @($env:RUSTUP_HOME, '/rustup')
    )
    foreach ($prefix in $prefixes) {
        if ($prefix[0]) {
            $releaseFlags += '--remap-path-prefix=' + $prefix[0] + '=' + $prefix[1]
            $releaseFlags += '--remap-path-prefix=' + $prefix[0].Replace('\','/') + '=' + $prefix[1]
        }
    }
    $env:CARGO_ENCODED_RUSTFLAGS = $releaseFlags -join [char]31
}
Push-Location $projectRoot
try {
    & $cargoExe @CargoArgs
    if ($LASTEXITCODE -ne 0) { throw "Cargo failed with exit code $LASTEXITCODE" }
} finally { Pop-Location; $env:CARGO_ENCODED_RUSTFLAGS = $previousEncodedFlags }

