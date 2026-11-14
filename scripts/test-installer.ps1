# Local native installer checks. No real models, devices, PATH registration or services.
[CmdletBinding()]
param([switch]$SkipRust)
$ErrorActionPreference = 'Stop'
$PSNativeCommandUseErrorActionPreference = $false
$repoRoot = [IO.Path]::GetFullPath((Join-Path $PSScriptRoot '..'))
Push-Location $repoRoot
$previousMode = $null
try {
    $native = Join-Path $repoRoot 'vosk-lib'
    foreach ($name in @('libvosk.dll','libgcc_s_seh-1.dll','libstdc++-6.dll','libwinpthread-1.dll')) {
        if (-not (Test-Path -LiteralPath (Join-Path $native $name) -PathType Leaf)) { throw "Missing native test dependency: $name. Run scripts/setup-vosk.ps1 first." }
    }
    $privateRust = Join-Path $repoRoot 'target/tools/rustup-native-ci'
    if (-not $env:RUSTUP_HOME -and (Test-Path -LiteralPath (Join-Path $privateRust 'toolchains'))) { $env:RUSTUP_HOME=$privateRust }
    if (-not $env:CARGO_HOME) { $env:CARGO_HOME=Join-Path $repoRoot 'target/tools/cargo-installer-ui' }
    if (-not $env:CARGO_TARGET_DIR) { $env:CARGO_TARGET_DIR=Join-Path $repoRoot 'target/native-ci-build' }
    $env:TEMP=Join-Path $repoRoot 'target/tmp'; $env:TMP=$env:TEMP
    New-Item -ItemType Directory -Path $env:TEMP -Force | Out-Null
    $env:PATH=$native+';'+(Join-Path $repoRoot 'target/tools')+';'+(Join-Path $env:CARGO_TARGET_DIR 'x86_64-pc-windows-msvc/debug')+';'+$env:PATH
    $env:RUSTFLAGS='-L native='+$native
    $env:PYTHONDONTWRITEBYTECODE='1'; $env:PYTHONIOENCODING='utf-8'
    if (-not ('LocalVoxInstallerTests.NativeErrors' -as [type])) {
        Add-Type -Namespace LocalVoxInstallerTests -Name NativeErrors -MemberDefinition '[System.Runtime.InteropServices.DllImport("kernel32.dll")] public static extern uint SetErrorMode(uint mode);'
    }
    $previousMode=[LocalVoxInstallerTests.NativeErrors]::SetErrorMode(3)
    if (-not $SkipRust) {
        & cargo test --locked -p localvox-light-core --lib env_file:: --target x86_64-pc-windows-msvc --jobs 2 -- --test-threads=2
        if ($LASTEXITCODE -ne 0) { throw 'Runtime configuration round-trip tests failed' }
        & cargo test --locked -p localvox-light --lib --bins --test setup --test install --target x86_64-pc-windows-msvc --jobs 2 -- --test-threads=2
        if ($LASTEXITCODE -ne 0) { throw 'Native installer tests failed' }
        & cargo test --locked -p localvox-setup-ui --target x86_64-pc-windows-msvc --jobs 2 -- --test-threads=1
        if ($LASTEXITCODE -ne 0) { throw 'Installer keyboard tests failed' }
    }
    & python -m unittest discover -s scripts/tests -p test_model_storage_delivery.py -v
    if ($LASTEXITCODE -ne 0) { throw 'Model storage fixtures failed' }
    & python -m unittest discover -s scripts/tests -p test_release_delivery.py -v
    if ($LASTEXITCODE -ne 0) { throw 'Release fixtures failed' }
    & python -m unittest discover -s scripts/tests -p test_bootstrap_delivery.py -v
    if ($LASTEXITCODE -ne 0) { throw 'Bootstrap fixtures failed' }
    & python -m unittest discover -s scripts/tests -p test_setup_tools_delivery.py -v
    if ($LASTEXITCODE -ne 0) { throw 'OS tool adapter fixtures failed' }
} finally {
    if ($null -ne $previousMode) { [void][LocalVoxInstallerTests.NativeErrors]::SetErrorMode($previousMode) }
    Pop-Location
}
