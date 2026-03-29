# Dev: vosk-lib + models under repo root (run from clone: .\scripts\setup-vosk.ps1).
# Standalone copy of this file: same folder as script becomes root (e.g. F:\kit\vosk-lib).
# User bundle: use install-release.ps1 with -InstallRoot, or -InstallRoot here.
#
#   -SkipModel  - native library only
#   -Force      - re-download model even if am/final.mdl exists
#   -InstallRoot "D:\path" - put vosk-lib + models there (absolute path recommended)
#
# Env: LOCALVOX_VOSK_API_TAG, LOCALVOX_SETUP_MODEL_URL, LOCALVOX_SETUP_FORCE=1
#
# UTF-8 BOM: required so Windows PowerShell 5.x reads this file as UTF-8.

param(
    [switch]$SkipModel,
    [switch]$Force,
    [string]$InstallRoot = ""
)

$ErrorActionPreference = "Stop"
$scriptDir = $PSScriptRoot
if (-not $scriptDir) {
    throw "PSScriptRoot is empty. Save as setup-vosk.ps1 and run: .\setup-vosk.ps1"
}

if ($InstallRoot) {
    $root = (Resolve-Path -LiteralPath $InstallRoot).Path
} elseif ((Split-Path -Leaf $scriptDir) -ieq "scripts") {
    $root = Split-Path -Parent $scriptDir
} else {
    $root = $scriptDir
}

$VoskTag = if ($env:LOCALVOX_VOSK_API_TAG) { $env:LOCALVOX_VOSK_API_TAG } else { "v0.3.42" }
$Ver = $VoskTag.TrimStart("v")
$ModelUrl = if ($env:LOCALVOX_SETUP_MODEL_URL) { $env:LOCALVOX_SETUP_MODEL_URL } else {
    "https://alphacephei.com/vosk/models/vosk-model-ru-0.42.zip"
}
if ($env:LOCALVOX_SETUP_FORCE -match "^(1|true|yes|on)$") { $Force = $true }

$arch = $env:PROCESSOR_ARCHITECTURE
if ($arch -eq "X86") {
    $zip = "vosk-win32-$Ver.zip"
} elseif ($arch -eq "ARM64") {
    Write-Warning "Windows ARM64: using vosk-win64 (x64); install manually if it fails."
    $zip = "vosk-win64-$Ver.zip"
} else {
    $zip = "vosk-win64-$Ver.zip"
}

$url = "https://github.com/alphacep/vosk-api/releases/download/$VoskTag/$zip"
Write-Host "Downloading $url"

$lib = Join-Path $root "vosk-lib"
$tmp = Join-Path ([System.IO.Path]::GetTempPath()) ([Guid]::NewGuid().ToString())
New-Item -ItemType Directory -Path $tmp | Out-Null
try {
    $zpath = Join-Path $tmp "vosk.zip"
    $ProgressPreference = "SilentlyContinue"
    Invoke-WebRequest -Uri $url -OutFile $zpath -UserAgent "localvox-light-setup/1.0"
    $ex = Join-Path $tmp "ex"
    Expand-Archive -Path $zpath -DestinationPath $ex -Force
    $inner = Get-ChildItem -Path $ex -Directory | Select-Object -First 1
    if (-not $inner) { throw "Empty vosk zip layout" }

    if (Test-Path $lib) { Remove-Item -Recurse -Force $lib }
    New-Item -ItemType Directory -Path $lib -Force | Out-Null
    Copy-Item -Path (Join-Path $inner.FullName "*") -Destination $lib -Recurse -Force
    New-Item -ItemType File -Path (Join-Path $lib ".gitkeep") -Force | Out-Null
    @(
        $VoskTag
        $zip
    ) | Set-Content -Path (Join-Path $lib ".vosk_native_version")
    Write-Host "Native library -> $lib"
} finally {
    Remove-Item -Recurse -Force $tmp -ErrorAction SilentlyContinue
}

if (-not $SkipModel) {
    $modelBase = [System.IO.Path]::GetFileNameWithoutExtension($ModelUrl)
    $dest = Join-Path $root "models\$modelBase"
    $finalMdl = Join-Path $dest "am\final.mdl"
    if ((Test-Path $finalMdl) -and -not $Force) {
        Write-Host "Model already present: $dest (skip). Re-download: -Force"
    } else {
        Write-Host "Downloading model $ModelUrl"
        $tmp2 = Join-Path ([System.IO.Path]::GetTempPath()) ([Guid]::NewGuid().ToString())
        New-Item -ItemType Directory -Path $tmp2 | Out-Null
        try {
            $mz = Join-Path $tmp2 "model.zip"
            Invoke-WebRequest -Uri $ModelUrl -OutFile $mz -UserAgent "localvox-light-setup/1.0"
            $modelsRoot = Join-Path $root "models"
            if ($Force -and (Test-Path $dest)) { Remove-Item -Recurse -Force $dest }
            New-Item -ItemType Directory -Path $modelsRoot -Force | Out-Null
            Expand-Archive -Path $mz -DestinationPath $modelsRoot -Force
            if (-not (Test-Path $finalMdl)) {
                throw "After extract, missing $dest\am\final.mdl (need full Vosk model zip)"
            }
            Write-Host "Model -> $dest"
        } finally {
            Remove-Item -Recurse -Force $tmp2 -ErrorAction SilentlyContinue
        }
    }
}

$modelPath = Join-Path $root "models\$([System.IO.Path]::GetFileNameWithoutExtension($ModelUrl))"
Write-Host ""
Write-Host "--- Next steps ---"
Write-Host "In .env:"
Write-Host ('  LOCALVOX_LIGHT_MODEL=' + $modelPath)
Write-Host "Add vosk-lib to PATH when running a standalone exe (DLL search path):"
Write-Host ('  ' + $lib)
