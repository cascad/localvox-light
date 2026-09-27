# Build the daemon and its worker together before starting either UI or daemon.
# Run from any directory; runtime paths and .env are resolved from the repository.
param(
    [ValidateSet('release', 'dev')]
    [string]$Profile = 'release',
    [switch]$Desktop,
    [switch]$BuildOnly
)

$ErrorActionPreference = 'Stop'
$repoDir = Split-Path -Parent $PSScriptRoot

Push-Location -LiteralPath $repoDir
try {
    $buildArgs = @(
        'build', '--locked', '--profile', $Profile,
        '-p', 'localvox-light', '-p', 'localvox-light-asr',
        '--bin', 'localvox-light', '--bin', 'localvox-process',
        '--message-format=json-render-diagnostics'
    )
    $required = @('localvox-light', 'localvox-process')
    if ($Desktop) {
        $buildArgs += @('-p', 'localvox-desktop', '--bin', 'localvox-desktop')
        $required += 'localvox-desktop'
    }

    # Cargo reports the actual paths, including custom target directories/targets.
    $built = @{}
    & cargo @buildArgs | ForEach-Object {
        $message = $_ | ConvertFrom-Json
        if ($message.reason -eq 'compiler-artifact' -and $message.executable) {
            $built[$message.target.name] = $message.executable
        }
    }
    if ($LASTEXITCODE -ne 0) {
        throw "Build failed (exit $LASTEXITCODE). LocalVox was not started."
    }

    $binaryDir = $null
    foreach ($name in $required) {
        $path = $built[$name]
        if (-not $path -or -not (Test-Path -LiteralPath $path -PathType Leaf)) {
            throw "Cargo did not produce $name. LocalVox was not started."
        }
        $parent = Split-Path -Parent $path
        if ($null -eq $binaryDir) { $binaryDir = $parent }
        if ($parent -ne $binaryDir) {
            throw "The binaries must be in the same directory: $path"
        }
        Write-Host "Built: $path"
    }
    if ($BuildOnly) { return }

    if ($Desktop) {
        & $built['localvox-desktop']
    } else {
        & $built['localvox-light'] --daemon
    }
    if ($LASTEXITCODE -ne 0) {
        throw "LocalVox exited with code $LASTEXITCODE."
    }
} finally {
    Pop-Location
}
