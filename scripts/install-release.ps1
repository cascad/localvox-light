# Установка localvox одной папкой (Windows).
#
#   cd $HOME\Desktop
#   $u='https://raw.githubusercontent.com/cascad/localvox-light/main/scripts/install-release.ps1'
#   $p="$env:TEMP\lv-install.ps1"; iwr -useb $u -OutFile $p; & $p
#
# Ключи ставятся ПОСЛЕ `& $p` (у Invoke-WebRequest их нет):  & $p -Tag v0.1.1 -RequiredOnly
#
# Что получится в <каталог>:
#   localvox-light.exe    демон: запись, варка, HTTP-интерфейс
#   localvox-process.exe  повар: расшифровка и сводка (БЕЗ НЕГО АРХИВ НЕ РАСШИФРУЕТСЯ)
#   localvox-desktop.exe  окно (если есть в релизе; иначе интерфейс в браузере)
#   libvosk.dll           нативная библиотека — грузится ДО main(), поэтому лежит рядом
#   models\               vosk-model-ru-0.42, gigaam-v3-e2e-ctc, diarize, ner-gliner
#   .env                  пути, прописанные абсолютно
#
# Порядок: бинари → модели → ПРОВЕРКА. Проверяет сам продукт (`localvox-light --doctor`), а не
# этот скрипт: демон знает, ГДЕ он ищет, а скрипт знает только то, что сам разложил. Эти два
# ответа уже расходились — модель на другом диске по LOCALVOX_LIGHT_MODEL скрипт считает
# отсутствующей, хотя всё работает.

param(
    [string]$InstallDir = "",
    [string]$Repo = "cascad/localvox-light",
    [string]$Tag = "latest",
    [string]$Branch = "main",
    [switch]$RequiredOnly,
    [switch]$SkipModels,
    [switch]$SkipBinary
)

$ErrorActionPreference = "Stop"
$ProgressPreference = 'SilentlyContinue'   # иначе Invoke-WebRequest режет скорость в разы

if ([string]::IsNullOrWhiteSpace($InstallDir)) {
    $InstallDir = Join-Path -Path (Get-Location).Path -ChildPath "localvox-light"
}
New-Item -ItemType Directory -Force -Path $InstallDir | Out-Null
$InstallDir = (Resolve-Path -LiteralPath $InstallDir).Path

$raw = "https://raw.githubusercontent.com/$Repo/$Branch/scripts"
$tmp = Join-Path ([System.IO.Path]::GetTempPath()) ("lv-" + [Guid]::NewGuid().ToString())
New-Item -ItemType Directory -Force -Path $tmp | Out-Null

function Say([string]$t) { Write-Host ''; Write-Host $t -ForegroundColor Cyan }

try {
    # ── Целевая тройка. Ошибиться — скачать чужой бинарь и получить «не является приложением
    #    Win32» вместо внятного сообщения.
    $target = switch ($env:PROCESSOR_ARCHITECTURE) {
        'AMD64' { 'x86_64-pc-windows-msvc' }
        'ARM64' { 'aarch64-pc-windows-msvc' }
        default { throw "Неизвестная архитектура: $env:PROCESSOR_ARCHITECTURE" }
    }

    # ── Нативная библиотека Vosk. Её грузит загрузчик Windows ДО main(), поэтому она обязана
    #    лежать рядом с exe: PATH из уже запущенного процесса — слишком поздно.
    Say "Нативная библиотека Vosk"
    $setupVosk = Join-Path $tmp 'setup-vosk.ps1'
    Invoke-WebRequest -Uri "$raw/setup-vosk.ps1" -OutFile $setupVosk -UserAgent 'localvox-install/1.0'
    & powershell -NoProfile -ExecutionPolicy Bypass -File $setupVosk -InstallRoot $InstallDir
    $voskLib = Join-Path $InstallDir 'vosk-lib'
    if (Test-Path -LiteralPath $voskLib) {
        Get-ChildItem -LiteralPath $voskLib -Filter '*.dll' -File -ErrorAction SilentlyContinue |
            ForEach-Object { Copy-Item -LiteralPath $_.FullName -Destination $InstallDir -Force }
    }

    # ── Бинари. ОДНИМ архивом на платформу: демон, повар и окно приезжают вместе. Раздельные
    #    файлы уже привели к установке без повара — продукт писал звук и не расшифровывал ничего.
    if (-not $SkipBinary) {
        Say "Бинари ($target)"
        $api = if ($Tag -eq 'latest') {
            "https://api.github.com/repos/$Repo/releases/latest"
        } else {
            "https://api.github.com/repos/$Repo/releases/tags/$Tag"
        }
        $rel = Invoke-RestMethod -Uri $api -Headers @{ 'User-Agent' = 'localvox-install/1.0' }
        $asset = @($rel.assets) | Where-Object { $_.name -like "*$target*" } | Select-Object -First 1
        if (-not $asset) {
            Write-Host "В релизе нет сборки под ${target}. Что есть:"
            @($rel.assets) | ForEach-Object { Write-Host "  -" $_.name }
            throw "Нет подходящего файла в релизе."
        }
        $zip = Join-Path $tmp $asset.name
        Write-Host "  ⇣ $($asset.name)"
        Invoke-WebRequest -Uri $asset.browser_download_url -OutFile $zip -UserAgent 'localvox-install/1.0'
        Expand-Archive -LiteralPath $zip -DestinationPath $InstallDir -Force

        if (-not (Test-Path -LiteralPath (Join-Path $InstallDir 'localvox-process.exe'))) {
            Write-Warning "В архиве нет localvox-process.exe — архив не будет расшифровываться"
        }
        # Windows помечает скачанное как «из интернета» (Zone.Identifier); на exe это даёт
        # предупреждение SmartScreen при каждом запуске. Снимаем: мы это только что скачали сами.
        Get-ChildItem -LiteralPath $InstallDir -Filter '*.exe' -File |
            ForEach-Object { Unblock-File -LiteralPath $_.FullName -ErrorAction SilentlyContinue }
    }

    # ── Модели. Список — в models.json, механика — в fetch-models.ps1. Оба кладём рядом,
    #    потому что скрипт ищет манифест возле себя.
    if (-not $SkipModels) {
        Say "Модели"
        $sdir = Join-Path $tmp 's'
        New-Item -ItemType Directory -Force -Path $sdir | Out-Null
        Invoke-WebRequest -Uri "$raw/models.json" -OutFile (Join-Path $sdir 'models.json') -UserAgent 'localvox-install/1.0'
        Invoke-WebRequest -Uri "$raw/fetch-models.ps1" -OutFile (Join-Path $sdir 'fetch-models.ps1') -UserAgent 'localvox-install/1.0'
        # НЕ $args: это автоматическая переменная PowerShell, и присваивание в неё внутри
        # скрипта работает не так, как читается.
        $fetchArgs = @('-Root', $InstallDir)
        if ($RequiredOnly) { $fetchArgs += '-RequiredOnly' }
        & powershell -NoProfile -ExecutionPolicy Bypass -File (Join-Path $sdir 'fetch-models.ps1') @fetchArgs
    }

    # ── .env. Пути АБСОЛЮТНЫЕ намеренно: под автозапуском рабочий каталог демона — system32,
    #    и относительный `models\…` не найдётся. Слэши прямые: обратные в .env съедаются
    #    как экранирование.
    Say "Настройки"
    $envPath = Join-Path $InstallDir '.env'
    if (Test-Path -LiteralPath $envPath) {
        Write-Host "  .env уже есть — не трогаю, правки ваши"
    } else {
        $d = $InstallDir -replace '\\', '/'
        $lines = @(
            "# Создано install-release.ps1. Пути абсолютные намеренно: под автозапуском рабочий",
            "# каталог демона — system32, и относительный models/... не найдётся.",
            "LOCALVOX_LIGHT_MODEL=$d/models/vosk-model-ru-0.42",
            "LOCALVOX_ASR_MODEL_DIR=$d/models/gigaam-v3-e2e-ctc",
            "LOCALVOX_LIGHT_AUDIO_DIR=$d/archive",
            "",
            "# Сводка и чистовик. Без Ollama расшифровка всё равно работает.",
            "LOCALVOX_LLM_BASE_URL=http://localhost:11434/v1",
            "LOCALVOX_LLM_MODEL=qwen3.5:9b",
            ""
        ) -join "`r`n"
        [System.IO.File]::WriteAllText($envPath, $lines, [System.Text.UTF8Encoding]::new($false))
        Write-Host "  записан $envPath"
    }

    # ── Проверка. Спрашиваем сам продукт.
    Say "Проверка"
    $exe = Join-Path $InstallDir 'localvox-light.exe'
    if (Test-Path -LiteralPath $exe) {
        Push-Location $InstallDir
        try { & $exe --doctor } finally { Pop-Location }
    } else {
        Write-Host "  бинаря нет — проверять нечего"
    }

    Say "Готово: $InstallDir"
    Write-Host "  Запуск:            cd `"$InstallDir`"; .\localvox-light.exe --daemon"
    Write-Host "  Проверить ещё раз: cd `"$InstallDir`"; .\localvox-light.exe --doctor"
    Write-Host "  Интерфейс:         http://127.0.0.1:3017/"
}
finally {
    Remove-Item -LiteralPath $tmp -Recurse -Force -ErrorAction SilentlyContinue
}
