# Скачивает vosk-lib и модель. Нужен PowerShell 5+ (Expand-Archive) или curl.exe.
# Запуск из корня репо: .\scripts\setup-vosk.ps1
#   -SkipModel  — только нативная библиотека
#   -Force      — перекачать модель, даже если am/final.mdl уже есть
#
# Переменные: $env:LOCALVOX_VOSK_API_TAG, $env:LOCALVOX_SETUP_MODEL_URL, $env:LOCALVOX_SETUP_FORCE=1

param(
    [switch]$SkipModel,
    [switch]$Force
)

$ErrorActionPreference = "Stop"
$root = Split-Path -Parent $PSScriptRoot
Set-Location $root

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
    Write-Warning "Windows ARM64: используется vosk-win64 (x64); при сбое ставьте библиотеку вручную."
    $zip = "vosk-win64-$Ver.zip"
} else {
    $zip = "vosk-win64-$Ver.zip"
}

$url = "https://github.com/alphacep/vosk-api/releases/download/$VoskTag/$zip"
Write-Host "Скачивание $url"

$tmp = Join-Path ([System.IO.Path]::GetTempPath()) ([Guid]::NewGuid().ToString())
New-Item -ItemType Directory -Path $tmp | Out-Null
try {
    $zpath = Join-Path $tmp "vosk.zip"
    $ProgressPreference = "SilentlyContinue"
    Invoke-WebRequest -Uri $url -OutFile $zpath -UserAgent "localvox-light-setup/1.0"
    $ex = Join-Path $tmp "ex"
    Expand-Archive -Path $zpath -DestinationPath $ex -Force
    $inner = Get-ChildItem -Path $ex -Directory | Select-Object -First 1
    if (-not $inner) { throw "Пустая структура архива vosk" }

    $lib = Join-Path $root "vosk-lib"
    if (Test-Path $lib) { Remove-Item -Recurse -Force $lib }
    New-Item -ItemType Directory -Path $lib -Force | Out-Null
    Copy-Item -Path (Join-Path $inner.FullName "*") -Destination $lib -Recurse -Force
    New-Item -ItemType File -Path (Join-Path $lib ".gitkeep") -Force | Out-Null
    @(
        $VoskTag
        $zip
    ) | Set-Content -Path (Join-Path $lib ".vosk_native_version")
    Write-Host "Нативная библиотека -> $lib"
} finally {
    Remove-Item -Recurse -Force $tmp -ErrorAction SilentlyContinue
}

if (-not $SkipModel) {
    $modelBase = [System.IO.Path]::GetFileNameWithoutExtension($ModelUrl)
    $dest = Join-Path $root "models\$modelBase"
    $finalMdl = Join-Path $dest "am\final.mdl"
    if ((Test-Path $finalMdl) -and -not $Force) {
        Write-Host "Модель уже есть: $dest (пропуск). Для перекачки: -Force"
    } else {
        Write-Host "Скачивание модели $ModelUrl"
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
                throw "После распаковки не найден $dest\am\final.mdl (нужен полный архив модели Vosk)"
            }
            Write-Host "Модель -> $dest"
        } finally {
            Remove-Item -Recurse -Force $tmp2 -ErrorAction SilentlyContinue
        }
    }
}

$modelPath = Join-Path $root "models\$([System.IO.Path]::GetFileNameWithoutExtension($ModelUrl))"
Write-Host ""
Write-Host "--- Дальше ---"
Write-Host "В .env:"
Write-Host "  LOCALVOX_LIGHT_MODEL=$modelPath"
Write-Host "Добавьте в PATH каталог с DLL для запуска exe:"
Write-Host "  $((Join-Path $root 'vosk-lib'))"
